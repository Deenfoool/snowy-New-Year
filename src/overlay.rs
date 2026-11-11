use crate::{
    icons,
    settings::Settings,
    shake::Shake,
    snowcap::SnowCap,
    snowball::{self, Snowball},
    weather::{self, Observation},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use windows_sys::{
    core::{w, GUID},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{
            LibraryLoader::GetModuleHandleW, Power::*, RemoteDesktop::*,
            Threading::GetCurrentThreadId,
        },
        UI::{Accessibility::*, Shell::*, WindowsAndMessaging::*},
    },
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};

const TRAY: u32 = WM_APP + 1;
const WAKE: u32 = WM_APP + 2;
const STOP: u32 = WM_APP + 3;
const OPEN: u32 = WM_APP + 4;
const GEOMETRY: u32 = WM_APP + 5;
const SCAN_TIMER: usize = 1;
const FRAME_TIMER: usize = 2;
const RESTORE_TIMER: usize = 3;
const CONTROLLER: windows_sys::core::PCWSTR = w!("SnowyNewYearController");
const SURFACE: windows_sys::core::PCWSTR = w!("SnowyNewYearSurface");
const DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

#[derive(Clone, Copy)]
pub enum Action {
    Open,
    Pause,
    Falling,
    Caps,
    Weather,
    Exit,
}

#[derive(Clone, Default)]
pub struct Diagnostics {
    pub tracked: usize,
    pub snowballs: usize,
    pub snowmen: usize,
    pub moving_balls: bool,
    pub caps: usize,
    pub bottom_edges: usize,
    pub particles: usize,
    pub fps: u32,
    pub reason: &'static str,
    pub monitors: Vec<(String, String)>,
}

pub struct Shared {
    pub settings: Settings,
    pub paused: bool,
    pub observation: Option<Observation>,
    pub diagnostics: Diagnostics,
    pub weather_error: bool,
}

pub struct Overlay {
    controller: usize,
    thread: Option<std::thread::JoinHandle<()>>,
    pub shared: Arc<Mutex<Shared>>,
    pub actions: mpsc::Receiver<Action>,
    pub tray_ready: bool,
}

impl Overlay {
    pub fn start(
        settings: Settings,
        observation: Option<Observation>,
        context: eframe::egui::Context,
    ) -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared {
            settings,
            paused: false,
            observation,
            diagnostics: Diagnostics::default(),
            weather_error: false,
        }));
        let (action_tx, actions) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let copy = shared.clone();
        let thread = std::thread::spawn(move || unsafe {
            if let Err(error) = run(copy, context, action_tx, &ready_tx) {
                let _ = ready_tx.send(Err(error));
            }
        });
        let (controller, tray_ready) = ready_rx.recv().map_err(|e| e.to_string())??;
        Ok(Self {
            controller,
            thread: Some(thread),
            shared,
            actions,
            tray_ready,
        })
    }

    pub fn wake(&self) {
        unsafe {
            PostMessageW(self.controller as HWND, WAKE, 0, 0);
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.controller as HWND, STOP, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn open_existing() {
    unsafe {
        let window = FindWindowW(CONTROLLER, null());
        if !window.is_null() {
            PostMessageW(window, OPEN, 0, 0);
        }
    }
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Copy)]
struct Pointer {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy)]
enum Grab {
    Cap(usize),
    Ball(u64),
}

#[derive(Clone, Copy)]
enum MouseAction {
    Down(Grab, Pointer),
    Move(Pointer),
    Up(Pointer),
    Decorate(u64),
}

#[derive(Clone)]
struct CapHit {
    id: usize,
    bounds: Bounds,
    heights: Vec<f32>,
    obstacles: Vec<Bounds>,
}

#[derive(Clone, Copy)]
struct BallHit {
    id: u64,
    x: f32,
    y: f32,
    radius: f32,
    decoratable: bool,
}

#[derive(Default)]
struct Events {
    controller: usize,
    actions: Vec<Action>,
    destroyed: Vec<usize>,
    dragging: Option<(usize, Instant)>,
    minimizing: HashMap<usize, Instant>,
    restore_pending: bool,
    locked: bool,
    display_off: bool,
    suspended: bool,
    geometry_pending: bool,
    displays_dirty: bool,
    restore_tray: bool,
    tray_message: u32,
    cap_hits: Vec<CapHit>,
    ball_hits: Vec<BallHit>,
    mouse: Vec<MouseAction>,
    mouse_capturing: bool,
    mouse_right_capturing: bool,
    last_mouse_wake: u32,
}

thread_local! { static EVENTS: RefCell<Events> = RefCell::new(Events::default()); }

fn queue_geometry() {
    let controller = EVENTS.with_borrow_mut(|events| {
        if events.geometry_pending {
            return 0;
        }
        events.geometry_pending = true;
        events.controller
    });
    if controller != 0 && unsafe { PostMessageW(controller as HWND, GEOMETRY, 0, 0) } == 0 {
        EVENTS.with_borrow_mut(|events| events.geometry_pending = false);
    }
}

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        STOP | WM_CLOSE => {
            PostQuitMessage(0);
        }
        OPEN => EVENTS.with_borrow_mut(|events| events.actions.push(Action::Open)),
        TRAY if lp as u32 == WM_LBUTTONDBLCLK => {
            EVENTS.with_borrow_mut(|events| events.actions.push(Action::Open));
        }
        TRAY if lp as u32 == WM_RBUTTONUP => {
            if let Some(action) = tray_menu(hwnd) {
                EVENTS.with_borrow_mut(|events| events.actions.push(action));
            }
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            EVENTS.with_borrow_mut(|events| events.displays_dirty = true);
            queue_geometry();
        }
        GEOMETRY => {}
        WM_WTSSESSION_CHANGE => EVENTS.with_borrow_mut(|events| match wp as u32 {
            WTS_SESSION_LOCK => events.locked = true,
            WTS_SESSION_UNLOCK => events.locked = false,
            _ => {}
        }),
        WM_POWERBROADCAST => {
            EVENTS.with_borrow_mut(|events| match wp as u32 {
                PBT_APMSUSPEND => events.suspended = true,
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => events.suspended = false,
                PBT_POWERSETTINGCHANGE if lp != 0 => {
                    let data = &*(lp as *const POWERBROADCAST_SETTING);
                    if data.PowerSetting.data1 == DISPLAY_STATE.data1 && data.DataLength >= 4 {
                        events.display_off = data.Data[0] == 0;
                    }
                }
                _ => {}
            });
            return 1;
        }
        _ => {
            let recreated = EVENTS.with_borrow_mut(|events| {
                let matched = msg != 0 && msg == events.tray_message;
                events.restore_tray |= matched;
                matched
            });
            if !recreated {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
        }
    }
    0
}

unsafe extern "system" fn surface_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        return HTTRANSPARENT as LRESULT;
    }
    if msg == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE as LRESULT;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe extern "system" fn win_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.is_null() || object != OBJID_WINDOW || child != 0 {
        return;
    }
    if !matches!(
        event,
        EVENT_OBJECT_CREATE
            | EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_SHOW
            | EVENT_OBJECT_HIDE
            | EVENT_OBJECT_REORDER
            | EVENT_OBJECT_LOCATIONCHANGE
            | EVENT_OBJECT_CLOAKED
            | EVENT_OBJECT_UNCLOAKED
            | EVENT_SYSTEM_FOREGROUND
            | EVENT_SYSTEM_MOVESIZESTART
            | EVENT_SYSTEM_MOVESIZEEND
            | EVENT_SYSTEM_MINIMIZESTART
            | EVENT_SYSTEM_MINIMIZEEND
    ) {
        return;
    }
    let owner = GetWindowThreadProcessId(hwnd, null_mut());
    if owner == GetCurrentThreadId()
        || event != EVENT_OBJECT_DESTROY && GetAncestor(hwnd, GA_ROOT) != hwnd
    {
        return;
    }
    let id = hwnd as usize;
    EVENTS.with_borrow_mut(|events| {
        if events.dragging.is_some_and(|(window, _)| window == id)
            && matches!(
                event,
                EVENT_SYSTEM_MOVESIZEEND | EVENT_OBJECT_DESTROY | EVENT_SYSTEM_MINIMIZESTART
            )
        {
            events.dragging = None;
        }
        match event {
            EVENT_SYSTEM_MOVESIZESTART => events.dragging = Some((id, Instant::now())),
            EVENT_OBJECT_DESTROY => {
                events.destroyed.push(id);
                events.minimizing.remove(&id);
            }
            EVENT_SYSTEM_MINIMIZESTART => {
                events.minimizing.insert(id, Instant::now());
            }
            EVENT_SYSTEM_MINIMIZEEND => {
                events.minimizing.remove(&id);
                events.restore_pending = true;
            }
            _ => {}
        }
    });
    queue_geometry();
}

/// Only intercept Shift+LMB dragging over visible snow and Shift+RMB on a
/// finished snowman's head. Never capture ordinary system mouse input.
unsafe extern "system" fn mouse_hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32
        || !matches!(
            wp as u32,
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE | WM_RBUTTONDOWN | WM_RBUTTONUP
        )
        || lp == 0
    {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let data = &*(lp as *const MSLLHOOKSTRUCT);
    let at = Pointer {
        x: data.pt.x as f32,
        y: data.pt.y as f32,
    };
    let mut consume = false;
    let mut wake = 0usize;
    EVENTS.with_borrow_mut(|events| {
        match wp as u32 {
            WM_RBUTTONDOWN
                if !events.mouse_right_capturing
                    && !events.mouse_capturing
                    && GetAsyncKeyState(VK_SHIFT as i32) < 0 =>
            {
                let target = events.ball_hits.iter().rev().find(|ball| {
                    let dx = at.x - ball.x;
                    let dy = at.y - ball.y;
                    ball.decoratable && dx * dx + dy * dy <= (ball.radius + 3.0).powi(2)
                });
                if let Some(ball) = target {
                    events.mouse_right_capturing = true;
                    events.mouse.push(MouseAction::Decorate(ball.id));
                    consume = true;
                    wake = events.controller;
                }
            }
            WM_RBUTTONUP if events.mouse_right_capturing => {
                // Swallow the corresponding release, too: underlying apps must
                // not see an orphan RMB-up or open their own context menu.
                events.mouse_right_capturing = false;
                consume = true;
            }
            WM_LBUTTONDOWN if !events.mouse_capturing && GetAsyncKeyState(VK_SHIFT as i32) < 0 => {
                let ball = events.ball_hits.iter().rev().find(|ball| {
                    let dx = at.x - ball.x;
                    let dy = at.y - ball.y;
                    dx * dx + dy * dy <= (ball.radius + 3.0).powi(2)
                });
                let target = ball.map(|ball| Grab::Ball(ball.id)).or_else(|| {
                    events.cap_hits.iter().rev().find(|hit| {
                        let local_x = (at.x - hit.bounds.left as f32).max(0.0);
                        let segment = local_x as usize / SNOW_SEGMENT;
                        let available = hit.heights.get(segment).copied().unwrap_or(0.0);
                        available >= 2.0
                            && at.x >= hit.bounds.left as f32
                            && at.x < hit.bounds.right as f32
                            && at.y < hit.bounds.top as f32
                            && at.y >= hit.bounds.top as f32 - available - 3.0
                            && !hit.obstacles.iter().any(|rect| {
                                rect.contains(at.x as i32, at.y as i32)
                            })
                    }).map(|hit| Grab::Cap(hit.id))
                });
                if let Some(target) = target {
                    events.mouse_capturing = true;
                    events.mouse.push(MouseAction::Down(target, at));
                    events.last_mouse_wake = data.time;
                    consume = true;
                    wake = events.controller;
                }
            }
            WM_MOUSEMOVE if events.mouse_capturing => {
                // Coalesce dense mouse motion so the message thread stays responsive.
                if let Some(MouseAction::Move(previous)) = events.mouse.last_mut() {
                    *previous = at;
                } else {
                    events.mouse.push(MouseAction::Move(at));
                }
                consume = true;
                if data.time.wrapping_sub(events.last_mouse_wake) >= 16 {
                    events.last_mouse_wake = data.time;
                    wake = events.controller;
                }
            }
            WM_LBUTTONUP if events.mouse_capturing => {
                events.mouse.push(MouseAction::Up(at));
                events.mouse_capturing = false;
                consume = true;
                wake = events.controller;
            }
            _ => {}
        }
    });
    if wake != 0 {
        PostMessageW(wake as HWND, WAKE, 0, 0);
    }
    if consume {
        1
    } else {
        CallNextHookEx(null_mut(), code, wp, lp)
    }
}

unsafe fn tray_menu(hwnd: HWND) -> Option<Action> {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return None;
    }
    for (id, label) in [
        (1, "Открыть приложение"),
        (2, "Пауза / продолжить"),
        (3, "Переключить снегопад"),
        (4, "Переключить снежные шапки"),
        (5, "Переключить режим погоды"),
    ] {
        AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, 6, w!("Полностью выйти"));
    let mut cursor: POINT = zeroed();
    GetCursorPos(&mut cursor);
    SetForegroundWindow(hwnd);
    let result = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        cursor.x,
        cursor.y,
        0,
        hwnd,
        null(),
    );
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
    match result {
        1 => Some(Action::Open),
        2 => Some(Action::Pause),
        3 => Some(Action::Falling),
        4 => Some(Action::Caps),
        5 => Some(Action::Weather),
        6 => Some(Action::Exit),
        _ => None,
    }
}

unsafe fn icon(instance: HINSTANCE, state: usize) -> HICON {
    let mut data = *icons::tray_badge(state);
    let mut mask = [0u8; 128];
    for (i, pixel) in data.chunks_exact_mut(4).enumerate() {
        if pixel[3] == 0 {
            mask[i / 8] |= 0x80 >> (i % 8);
        }
        pixel.swap(0, 2);
    }
    let size = icons::BADGE_SIZE as i32;
    CreateIcon(instance, size, size, 1, 32, mask.as_ptr(), data.as_ptr())
}

fn tip(data: &mut NOTIFYICONDATAW, state: u8) {
    let title = match state {
        1 => "Snowy New Year · пауза",
        2 => "Snowy New Year · ошибка погоды",
        _ => "Snowy New Year · эффекты включены",
    };
    data.szTip.fill(0);
    for (slot, value) in data.szTip.iter_mut().zip(title.encode_utf16()) {
        *slot = value;
    }
}

unsafe fn run(
    shared: Arc<Mutex<Shared>>,
    ctx: eframe::egui::Context,
    actions: mpsc::Sender<Action>,
    ready: &mpsc::SyncSender<Result<(usize, bool), String>>,
) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    for (class, proc) in [
        (CONTROLLER, controller_proc as WindowProc),
        (SURFACE, surface_proc),
    ] {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class,
            ..zeroed()
        };
        if RegisterClassW(&class) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        CONTROLLER,
        w!("Snowy New Year · Engine"),
        WS_POPUP,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if hwnd.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let tray_message = RegisterWindowMessageW(w!("TaskbarCreated"));
    EVENTS.with_borrow_mut(|events| {
        events.controller = hwnd as usize;
        events.tray_message = tray_message;
    });
    let icons = [icon(instance, 0), icon(instance, 1), icon(instance, 2)];
    let mut notify: NOTIFYICONDATAW = zeroed();
    notify.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    notify.hWnd = hwnd;
    notify.uID = 1;
    notify.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    notify.uCallbackMessage = TRAY;
    notify.hIcon = icons[0];
    tip(&mut notify, 0);
    let tray_ready = Shell_NotifyIconW(NIM_ADD, &notify) != 0;
    let mut tray_state = 0;
    let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), instance, 0);
    if mouse_hook.is_null() {
        crate::settings::log("Snowballs: WH_MOUSE_LL hook unavailable");
    }
    let hooks = [
        (EVENT_OBJECT_CREATE, EVENT_OBJECT_LOCATIONCHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    ]
    .map(|(first, last)| {
        let hook = SetWinEventHook(
            first,
            last,
            null_mut(),
            Some(win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNTHREAD,
        );
        if hook.is_null() {
            crate::settings::log(&format!("SetWinEventHook failed: {first:#x}..{last:#x}"));
        }
        hook
    });
    WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
    let power = RegisterPowerSettingNotification(hwnd, &DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
    SetTimer(hwnd, SCAN_TIMER, 300, None);
    let mut engine = Engine::new(instance);
    engine.scan_displays();
    engine.scan_windows();
    let _ = ready.send(Ok((hwnd as usize, tray_ready)));
    PostMessageW(hwnd, WAKE, 0, 0);
    let mut message: MSG = zeroed();
    let mut frame_interval = 0;
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        DispatchMessageW(&message);
        let (actions_now, sleeping, restore_tray) = EVENTS.with_borrow_mut(|events| {
            (
                std::mem::take(&mut events.actions),
                events.locked || events.display_off || events.suspended,
                std::mem::take(&mut events.restore_tray),
            )
        });
        for action in actions_now {
            let _ = actions.send(action);
            ctx.request_repaint();
        }
        let slow_tick = message.message == WM_TIMER && message.wParam == SCAN_TIMER;
        let restored = message.message == WM_TIMER && message.wParam == RESTORE_TIMER;
        if restored {
            KillTimer(hwnd, RESTORE_TIMER);
        }
        let geometry = message.message == GEOMETRY;
        let reconcile = slow_tick && engine.last_reconcile.elapsed() >= Duration::from_secs(2);
        if geometry || restored || reconcile {
            let (destroyed, displays_dirty, restore_pending) = EVENTS.with_borrow_mut(|events| {
                if geometry {
                    events.geometry_pending = false;
                }
                (
                    std::mem::take(&mut events.destroyed),
                    std::mem::take(&mut events.displays_dirty),
                    std::mem::take(&mut events.restore_pending),
                )
            });
            for id in destroyed {
                engine.caps.remove(&id);
            }
            if displays_dirty || reconcile {
                engine.scan_displays();
            }
            if reconcile {
                engine.last_reconcile = Instant::now();
            }
            engine.scan_windows();
            if restore_pending {
                SetTimer(hwnd, RESTORE_TIMER, 32, None);
            }
        }
        let now = Instant::now();
        let wake = message.message == WAKE;
        if wake {
            engine.changed = true;
        }
        let frame = message.message == WM_TIMER && message.wParam == FRAME_TIMER;
        if !(slow_tick
            || frame
            || wake
            || restore_tray
            || geometry
            || restored
            || message.message == WM_POWERBROADCAST
            || message.message == WM_WTSSESSION_CHANGE)
        {
            continue;
        }
        let (settings, paused, observation, weather_error) = {
            let state = shared.lock().unwrap();
            (
                state.settings.clone(),
                state.paused,
                state.observation.clone(),
                state.weather_error,
            )
        };
        let desired = if paused {
            1
        } else if weather_error {
            2
        } else {
            0
        };
        if tray_state != desired || restore_tray {
            tray_state = desired;
            notify.hIcon = icons[desired as usize];
            tip(&mut notify, desired);
            Shell_NotifyIconW(if restore_tray { NIM_ADD } else { NIM_MODIFY }, &notify);
        }
        let mouse_actions = EVENTS.with_borrow_mut(|events| std::mem::take(&mut events.mouse));
        for action in mouse_actions {
            engine.handle_mouse(action, now, &settings);
        }
        let conditions = weather::conditions(&settings, observation.as_ref(), now);
        let expiry = observation.as_ref().map(Observation::expires);
        let diag = engine.tick(
            now,
            &settings,
            conditions,
            expiry,
            paused,
            sleeping,
            frame || wake,
        );
        let desired = if diag.particles > 0 || diag.moving_balls {
            1000u32.div_ceil(settings.fps).max(10)
        } else {
            0
        };
        if desired != frame_interval {
            KillTimer(hwnd, FRAME_TIMER);
            if desired != 0 {
                SetTimer(hwnd, FRAME_TIMER, desired, None);
            }
            frame_interval = desired;
        }
        shared.lock().unwrap().diagnostics = diag;
    }
    Shell_NotifyIconW(NIM_DELETE, &notify);
    for icon in icons {
        if !icon.is_null() {
            DestroyIcon(icon);
        }
    }
    for hook in hooks {
        if !hook.is_null() {
            UnhookWinEvent(hook);
        }
    }
    if !mouse_hook.is_null() {
        UnhookWindowsHookEx(mouse_hook);
    }
    if power != 0 {
        UnregisterPowerSettingNotification(power);
    }
    WTSUnRegisterSessionNotification(hwnd);
    DestroyWindow(hwnd);
    Ok(())
}

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Bounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl From<RECT> for Bounds {
    fn from(r: RECT) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}

impl Bounds {
    fn width(self) -> i32 {
        self.right - self.left
    }
    fn height(self) -> i32 {
        self.bottom - self.top
    }
    fn contains(self, x: i32, y: i32) -> bool {
        self.left <= x && x < self.right && self.top <= y && y < self.bottom
    }
    fn intersects(self, b: Self) -> bool {
        self.left < b.right && b.left < self.right && self.top < b.bottom && b.top < self.bottom
    }
    fn covers(self, b: Self) -> bool {
        self.left <= b.left + 2
            && self.right >= b.right - 2
            && self.top <= b.top + 2
            && self.bottom >= b.bottom - 2
    }
}

struct Display {
    bounds: Bounds,
    work_area: Bounds,
    name: String,
}

unsafe extern "system" fn enumerate_display(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let displays = &mut *(data as *mut Vec<Display>);
    let mut info: MONITORINFOEXW = zeroed();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(monitor, &mut info.monitorInfo) != 0 {
        let end = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        displays.push(Display {
            bounds: info.monitorInfo.rcMonitor.into(),
            work_area: info.monitorInfo.rcWork.into(),
            name: String::from_utf16_lossy(&info.szDevice[..end]),
        });
    }
    1
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Window {
    id: usize,
    process: u32,
    thread: u32,
    bounds: Bounds,
    visible: bool,
    cap: bool,
    maximized: bool,
}

impl Window {
    fn shows_cap(self, displays: &[Display]) -> bool {
        self.cap
            && self.visible
            && !self.maximized
            && !displays.iter().any(|display| {
                self.bounds.covers(display.bounds) || self.bounds.covers(display.work_area)
            })
    }
}

unsafe extern "system" fn enumerate_window(hwnd: HWND, data: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if style & WS_CHILD != 0 || ex_style & WS_EX_TRANSPARENT != 0 {
        return 1;
    }
    let mut class = [0u16; 128];
    let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
    let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
    if matches!(
        class.as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "SnowyNewYearSurface"
            | "SnowyNewYearController"
    ) {
        return 1;
    }
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        (&mut cloaked as *mut u32).cast(),
        4,
    );
    let visible = IsIconic(hwnd) == 0 && cloaked == 0;
    let mut rect: RECT = zeroed();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        (&mut rect as *mut RECT).cast(),
        size_of::<RECT>() as u32,
    ) < 0
    {
        GetWindowRect(hwnd, &mut rect);
    }
    let bounds = Bounds::from(rect);
    let mut process = 0;
    let thread = GetWindowThreadProcessId(hwnd, &mut process);
    if thread != 0 {
        let cap = ex_style & WS_EX_TOOLWINDOW == 0
            && (style & WS_CAPTION == WS_CAPTION || GetWindowTextLengthW(hwnd) > 0);
        (&mut *(data as *mut Vec<Window>)).push(Window {
            id: hwnd as usize,
            process,
            thread,
            bounds,
            visible,
            cap,
            maximized: IsZoomed(hwnd) != 0,
        });
    }
    1
}

const SNOW_SEGMENT: usize = 10;

struct Cap {
    process: u32,
    thread: u32,
    seed: u32,
    snow: SnowDepth,
    /// Amount of snow already rolled away in each 10px section.
    removed: Vec<f32>,
    removed_updated: Instant,
    ice_factor: f32,
    ice_updated: Instant,
    bitmap: Option<CapBitmap>,
}

struct CapBitmap {
    shape: SnowCap,
    pixels: Vec<(i32, i32, u32)>,
}

struct SnowDepth {
    height: f32,
    updated: Instant,
}

impl SnowDepth {
    fn height_at(&self, now: Instant, rate: f32, limit: f32) -> f32 {
        let seconds = now.saturating_duration_since(self.updated).as_secs_f32();
        (self.height + seconds * rate).clamp(0.0, limit)
    }

    fn advance(&mut self, end: Instant, now: Instant, rate: f32, old_limit: f32, new_limit: f32) {
        self.height = self.height_at(end, rate, old_limit).min(new_limit);
        self.updated = now;
    }
}

impl CapBitmap {
    fn pixels<'a>(cache: &'a mut Option<Self>, shape: SnowCap, removed: &[f32]) -> &'a [(i32, i32, u32)] {
        let bitmap = cache.get_or_insert_with(|| Self {
            shape,
            pixels: Vec::new(),
        });
        if bitmap.shape != shape || bitmap.pixels.is_empty() {
            bitmap.shape = shape;
            bitmap.pixels.clear();
            shape.rasterize_removed(0..shape.width, shape.rows(), removed, SNOW_SEGMENT, |x, y, pixel| {
                bitmap.pixels.push((x, y, pixel));
            });
        }
        &bitmap.pixels
    }
}

#[derive(Clone, Copy)]
enum Gesture {
    Rolling {
        window: usize,
        ball: u64,
        sampled: Pointer,
    },
    Dragging {
        ball: u64,
    },
}

struct Engine {
    instance: HINSTANCE,
    windows: Vec<Window>,
    caps: HashMap<usize, Cap>,
    balls: Vec<Snowball>,
    next_ball: u64,
    gesture: Option<Gesture>,
    last_ball_physics: Instant,
    drips: Vec<WaterDrip>,
    last_drip: Instant,
    displays: Vec<Display>,
    surfaces: HashMap<String, Canvas>,
    random: u32,
    drag: Option<WindowDrag>,
    loose_snow: Vec<LooseSnow>,
    bottom_snow: SnowDepth,
    last_reconcile: Instant,
    last_animation: Instant,
    rate: f32,
    bottom_rate: f32,
    expiry: Option<Instant>,
    max_height: f32,
    changed: bool,
    frames: u32,
    fps: u32,
    fps_since: Instant,
}

struct WindowDrag {
    session: (usize, Instant),
    size: (i32, i32),
    shake: Option<Shake>,
}

const SHED_TIME: f32 = 1.05;

struct LooseSnow {
    start: Instant,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: f32,
}

impl LooseSnow {
    fn age(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32()
    }

    fn sample(&self, now: Instant) -> SnowParticle {
        let t = self.age(now);
        SnowParticle {
            x: self.x + self.vx * t,
            y: self.y + self.vy * t + 265.0 * t * t,
            radius: self.size * (1.0 - 0.30 * t),
            opacity: (1.0 - t / SHED_TIME).min(0.96) * 255.0,
        }
    }
}

struct SnowParticle {
    x: f32,
    y: f32,
    radius: f32,
    opacity: f32,
}

struct WaterDrip {
    start: Instant,
    x: f32,
    y: f32,
    wind: f32,
}

impl WaterDrip {
    fn age(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32()
    }

    fn sample(&self, now: Instant) -> SnowParticle {
        let t = self.age(now);
        SnowParticle {
            x: self.x + self.wind * 12.0 * t,
            y: self.y + 45.0 * t + 170.0 * t * t,
            radius: 1.5,
            opacity: (225.0 * (1.0 - t / 2.5)).clamp(0.0, 225.0),
        }
    }
}

fn random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

impl Engine {
    fn new(instance: HINSTANCE) -> Self {
        let now = Instant::now();
        Self {
            instance,
            windows: Vec::new(),
            caps: HashMap::new(),
            balls: Vec::new(),
            next_ball: 1,
            gesture: None,
            last_ball_physics: now,
            drips: Vec::new(),
            last_drip: now,
            displays: Vec::new(),
            surfaces: HashMap::new(),
            random: 0xc41f_543e,
            drag: None,
            loose_snow: Vec::new(),
            bottom_snow: SnowDepth {
                height: 0.0,
                updated: now,
            },
            last_reconcile: now,
            last_animation: now,
            rate: 0.0,
            bottom_rate: 0.0,
            expiry: None,
            max_height: 18.0,
            changed: true,
            frames: 0,
            fps: 0,
            fps_since: now,
        }
    }

    unsafe fn scan_windows(&mut self) {
        let now = Instant::now();
        let mut windows: Vec<Window> = Vec::new();
        EnumWindows(
            Some(enumerate_window),
            (&mut windows as *mut Vec<Window>) as LPARAM,
        );
        EVENTS.with_borrow_mut(|events| {
            events.minimizing.retain(|_, started| {
                now.saturating_duration_since(*started) < Duration::from_millis(450)
            });
            for window in &mut windows {
                window.visible &= !events.minimizing.contains_key(&window.id);
            }
        });
        self.changed |= self.windows != windows;
        self.windows = windows;
        self.caps.retain(|id, cap| {
            // Preserve hidden windows. Destruction invalidates this entry via the event hook.
            let mut process = 0;
            let thread = GetWindowThreadProcessId(*id as HWND, &mut process);
            process == cap.process && thread == cap.thread
        });
        for window in self.windows.iter().filter(|window| window.cap) {
            let cap = self.caps.entry(window.id).or_insert_with(|| Cap {
                process: window.process,
                thread: window.thread,
                seed: (random(&mut self.random) * 16_777_216.0) as u32,
                snow: SnowDepth {
                    height: 0.0,
                    updated: now,
                },
                removed: Vec::new(),
                removed_updated: now,
                ice_factor: 0.0,
                ice_updated: now,
                bitmap: None,
            });
            let wanted = (window.bounds.width().max(0) as usize).div_ceil(SNOW_SEGMENT);
            if cap.removed.len() != wanted {
                cap.removed.resize(wanted, 0.0);
                cap.bitmap = None;
            }
        }
    }

    unsafe fn scan_displays(&mut self) {
        self.displays.clear();
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(enumerate_display),
            (&mut self.displays as *mut Vec<Display>) as LPARAM,
        );
        self.surfaces.retain(|name, canvas| {
            self.displays
                .iter()
                .any(|display| display.name == *name && display.bounds == canvas.bounds)
        });
    }

    fn handle_mouse(&mut self, action: MouseAction, now: Instant, settings: &Settings) {
        if !settings.snowballs {
            self.gesture = None;
            return;
        }
        match action {
            MouseAction::Decorate(id) => {
                if settings.snowman_decorations && snowball::cycle_outfit(&mut self.balls, id) {
                    self.changed = true;
                }
            }
            MouseAction::Down(target, at) => {
                if self.gesture.is_some() {
                    return;
                }
                match target {
                    Grab::Cap(window) => {
                        if self.balls.len() >= snowball::MAX_BALLS
                            || !self.caps.contains_key(&window)
                            || !self.windows.iter().any(|w| w.id == window && w.shows_cap(&self.displays))
                        {
                            return;
                        }
                        let id = self.next_ball;
                        self.next_ball = self.next_ball.wrapping_add(1).max(1);
                        self.balls.push(Snowball::new(id, at.x, at.y));
                        self.collect_snow(window, id, at, at, now, settings);
                        self.gesture = Some(Gesture::Rolling {
                            window,
                            ball: id,
                            sampled: at,
                        });
                        self.changed = true;
                    }
                    Grab::Ball(id) => {
                        let Some(ball) = self.balls.iter_mut().find(|ball| ball.id == id) else {
                            return;
                        };
                        ball.parent = None;
                        ball.grounded = false;
                        ball.vx = 0.0;
                        ball.vy = 0.0;
                        snowball::move_with_children(&mut self.balls, id, at.x, at.y);
                        self.gesture = Some(Gesture::Dragging { ball: id });
                        self.changed = true;
                    }
                }
            }
            MouseAction::Move(at) | MouseAction::Up(at) => {
                let released = matches!(action, MouseAction::Up(_));
                let Some(gesture) = self.gesture.take() else {
                    return;
                };
                match gesture {
                    Gesture::Rolling { window, ball, sampled } => {
                        snowball::move_with_children(&mut self.balls, ball, at.x, at.y);
                        let sampled = if (at.x - sampled.x).abs() >= 8.0
                            || (at.y - sampled.y).abs() >= 12.0
                        {
                            self.collect_snow(window, ball, sampled, at, now, settings);
                            at
                        } else {
                            sampled
                        };
                        if released {
                            self.finish_ball(ball);
                        } else {
                            self.gesture = Some(Gesture::Rolling { window, ball, sampled });
                        }
                        self.changed = true;
                    }
                    Gesture::Dragging { ball } => {
                        snowball::move_with_children(&mut self.balls, ball, at.x, at.y);
                        if released {
                            self.finish_ball(ball);
                        } else {
                            self.gesture = Some(Gesture::Dragging { ball });
                        }
                        self.changed = true;
                    }
                }
            }
        }
    }

    fn finish_ball(&mut self, id: u64) {
        if self.balls.iter().find(|ball| ball.id == id).is_some_and(|ball| !ball.is_formed()) {
            self.balls.retain(|ball| ball.id != id);
            return;
        }
        if snowball::try_stack(&mut self.balls, id) {
            return;
        }
        if let Some(ball) = self.balls.iter_mut().find(|ball| ball.id == id) {
            ball.grounded = false;
            ball.vx = 0.0;
            ball.vy = 0.0;
        }
    }

    fn collect_snow(
        &mut self,
        window_id: usize,
        ball_id: u64,
        from: Pointer,
        to: Pointer,
        now: Instant,
        settings: &Settings,
    ) {
        let Some(window) = self.windows.iter().find(|w| w.id == window_id) else {
            return;
        };
        let bounds = window.bounds;
        let Some(cap) = self.caps.get_mut(&window_id) else {
            return;
        };
        let height = cap.snow.height_at(now, self.rate, self.max_height);
        if height < 2.0 || bounds.width() <= 0 {
            return;
        }
        let shape = SnowCap {
            width: bounds.width(),
            depth: height,
            roughness: settings.roughness / 100.0,
            seed: cap.seed,
            icicles: settings.icicles,
            ice_factor: (cap.ice_factor * 20.0).round() / 20.0,
            wind_bias: 0.0,
        };
        let distance = ((to.x - from.x).abs().max((to.y - from.y).abs()) / 7.0)
            .ceil()
            .clamp(1.0, 200.0) as usize;
        let mut last_segment = None;
        let mut collected = 0.0;
        for step in 0..=distance {
            let t = step as f32 / distance as f32;
            let x = from.x + (to.x - from.x) * t;
            let y = from.y + (to.y - from.y) * t;
            if x < bounds.left as f32
                || x >= bounds.right as f32
                || y < bounds.top as f32 - height - 9.0
                || y > bounds.top as f32 + 4.0
            {
                continue;
            }
            let local_x = (x as i32 - bounds.left).max(0);
            let segment = local_x as usize / SNOW_SEGMENT;
            if last_segment == Some(segment) {
                continue;
            }
            last_segment = Some(segment);
            let Some(removed) = cap.removed.get_mut(segment) else {
                continue;
            };
            let middle = (segment * SNOW_SEGMENT + SNOW_SEGMENT / 2)
                .min(bounds.width() as usize - 1) as f32;
            let available = (shape.sample_height(middle) - *removed).max(0.0);
            let taken = available.min(3.5);
            *removed += taken;
            collected += taken * SNOW_SEGMENT as f32;
        }
        if collected > 0.0 {
            cap.removed_updated = now;
            cap.bitmap = None;
            if let Some(ball) = self.balls.iter_mut().find(|ball| ball.id == ball_id) {
                ball.collect(collected);
            }
            self.changed = true;
        }
    }

    fn advance_balls(&mut self, now: Instant, paused: bool, sleeping: bool, settings: &Settings) {
        let dt = now.saturating_duration_since(self.last_ball_physics).as_secs_f32().min(0.05);
        self.last_ball_physics = now;
        if paused || sleeping || !settings.snowballs || dt <= 0.0 {
            return;
        }
        let moving = self.gesture.map(|g| match g {
            Gesture::Rolling { ball, .. } | Gesture::Dragging { ball } => ball,
        });
        let ids: Vec<u64> = self.balls.iter()
            .filter(|ball| ball.parent.is_none() && !ball.grounded && Some(ball.id) != moving)
            .map(|ball| ball.id)
            .collect();
        for id in ids {
            let Some(ball) = self.balls.iter().find(|ball| ball.id == id).copied() else {
                continue;
            };
            let display = self.displays.iter().find(|d| {
                ball.x >= d.bounds.left as f32 && ball.x < d.bounds.right as f32
            });
            let Some(display) = display else {
                continue;
            };
            if settings.fullscreen_pause && self.windows.iter()
                .any(|w| w.visible && w.bounds.covers(display.bounds))
            {
                continue;
            }
            let vy = ball.vy + snowball::GRAVITY * dt;
            let mut y = ball.y + vy * dt;
            // Stand on the usable desktop, above the taskbar.
            let floor = display.work_area.bottom as f32 - ball.radius() - 1.0;
            if y >= floor {
                y = floor;
            }
            snowball::move_with_children(&mut self.balls, id, ball.x, y);
            if let Some(moved) = self.balls.iter_mut().find(|b| b.id == id) {
                moved.vy = if y >= floor { 0.0 } else { vy };
                moved.grounded = y >= floor;
            }
            self.changed = true;
        }
    }

    /// Ice changes continuously with elapsed time, but the rasterizer sees
    /// only 5% steps, so a large collection of open windows stays cheap.
    fn advance_ice(&mut self, now: Instant, s: &Settings, temp: f32) {
        for cap in self.caps.values_mut() {
            let elapsed = now.saturating_duration_since(cap.ice_updated)
                .as_secs_f32().min(6.0 * 3600.0);
            cap.ice_updated = now;
            let old_step = (cap.ice_factor * 20.0).round() as i32;
            let depth = cap.snow.height_at(now, self.rate, self.max_height);
            if !s.icicles {
                cap.ice_factor = 0.0;
            } else if temp <= 0.0 && depth >= 5.0 {
                let growth = (1.0 + temp.abs() * 0.07).min(2.5);
                cap.ice_factor = (cap.ice_factor + elapsed * growth / 180.0).min(1.0);
            } else {
                let melting = (1.0 + temp.max(0.0) * 0.25).min(4.0);
                cap.ice_factor = (cap.ice_factor - elapsed * melting / 100.0).max(0.0);
            }
            if old_step != (cap.ice_factor * 20.0).round() as i32 {
                self.changed = true;
            }
        }
    }

    fn update_drips(
        &mut self,
        now: Instant,
        s: &Settings,
        weather: weather::Conditions,
        paused: bool,
        sleeping: bool,
    ) {
        self.drips.retain(|drop| drop.age(now) < 2.5);
        if !s.icicles || !s.icicle_drips {
            self.drips.clear();
            return;
        }
        if paused || sleeping || weather.temperature <= 0.0
            || self.drips.len() >= 48
            || now.saturating_duration_since(self.last_drip) < Duration::from_millis(650)
        {
            return;
        }
        self.last_drip = now;
        // Try a handful of existing, visible window edges. No physical
        // particles are created for fullscreen/hidden desktops.
        let candidates: Vec<_> = self.windows.iter().copied()
            .filter(|window| window.shows_cap(&self.displays))
            .collect();
        if candidates.is_empty() {
            return;
        }
        let start = ((random(&mut self.random) * candidates.len() as f32) as usize)
            .min(candidates.len() - 1);
        for index in 0..candidates.len().min(8) {
            let window = candidates[(start + index) % candidates.len()];
            let Some(display) = self.displays.iter().find(|display| {
                display.bounds.intersects(window.bounds)
                    && (s.monitor.is_empty() || s.monitor == display.name)
            }) else { continue };
            if s.fullscreen_pause
                && self.windows.iter().any(|w| w.visible && w.bounds.covers(display.bounds))
            {
                continue;
            }
            let Some(cap) = self.caps.get(&window.id) else { continue };
            if cap.ice_factor < 0.10 { continue; }
            let depth = cap.snow.height_at(now, self.rate, self.max_height);
            if depth < 5.0 || window.bounds.width() <= 0 { continue; }
            let shape = SnowCap {
                width: window.bounds.width(),
                depth,
                roughness: s.roughness / 100.0,
                seed: cap.seed,
                icicles: true,
                ice_factor: (cap.ice_factor * 20.0).round() / 20.0,
                wind_bias: 0.0,
            };
            let cells = (window.bounds.width().max(1) as usize).div_ceil(52);
            for _ in 0..6 {
                let cell = (random(&mut self.random) * cells as f32) as u32;
                if let Some((x, tip)) = shape.icicle_tip_in_cell(cell) {
                    self.drips.push(WaterDrip {
                        start: now,
                        x: window.bounds.left as f32 + x,
                        y: window.bounds.top as f32 + tip,
                        wind: weather.wind,
                    });
                    self.changed = true;
                    return;
                }
            }
        }
    }

    fn update_shake(&mut self, now: Instant, settings: &Settings, suspended: bool) {
        let Some(session) = EVENTS.with_borrow(|events| events.dragging) else {
            self.drag = None;
            return;
        };
        let target = self.windows.iter().find(|window| window.id == session.0);
        let Some(window) = target
            .filter(|window| {
                !suspended
                    && settings.caps
                    && settings.shake_to_clear
                    && window.shows_cap(&self.displays)
            })
            .copied()
        else {
            self.drag = None;
            return;
        };
        let r = window.bounds;
        let size = (r.width(), r.height());
        if self
            .drag
            .as_ref()
            .is_none_or(|drag| drag.session != session)
        {
            self.drag = Some(WindowDrag {
                session,
                size,
                shake: Some(Shake::new(r.left, r.top, now)),
            });
        }
        let drag = self.drag.as_mut().unwrap();
        if drag.size != size {
            drag.shake = None;
        }
        let Some(direction) = drag
            .shake
            .as_mut()
            .and_then(|shake| shake.observe(r.left, r.top, now))
        else {
            return;
        };
        let Some(cap) = self.caps.get_mut(&window.id) else {
            return;
        };
        let depth = cap.snow.height_at(now, self.rate, self.max_height);
        if depth < 1.0 {
            return;
        }
        cap.snow = SnowDepth {
            height: 0.0,
            updated: now,
        };
        cap.bitmap = None;
        self.changed = true;
        let count = ((r.width() as f32 / 20.0) * (depth / 12.0).sqrt()).clamp(10.0, 52.0) as usize;
        for i in 0..count.min(128usize.saturating_sub(self.loose_snow.len())) {
            let spread = (i as f32 + random(&mut self.random)) / count as f32;
            self.loose_snow.push(LooseSnow {
                start: now,
                x: r.left as f32 + spread * r.width() as f32,
                y: r.top as f32 - random(&mut self.random) * depth * 0.68,
                vx: direction as f32 * 72.0 + (random(&mut self.random) - 0.5) * 190.0,
                vy: -18.0 - random(&mut self.random) * 94.0,
                size: 1.3 + random(&mut self.random) * (depth * 0.24).clamp(1.2, 4.8),
            });
        }
    }

    fn tick(
        &mut self,
        now: Instant,
        s: &Settings,
        weather: weather::Conditions,
        expiry: Option<Instant>,
        paused: bool,
        sleeping: bool,
        animate: bool,
    ) -> Diagnostics {
        let dt = if animate {
            let elapsed = now
                .saturating_duration_since(self.last_animation)
                .as_secs_f32()
                .min(0.1);
            self.last_animation = now;
            elapsed
        } else {
            0.0
        };
        self.advance_balls(now, paused, sleeping, s);
        let snowfall =
            weather.snow * s.accumulation * 0.85 - weather.temperature.max(0.0) * s.melt * 0.035;
        let rate = if s.caps { snowfall } else { 0.0 };
        let bottom_rate = if s.bottom_snow { snowfall } else { 0.0 };
        let expiry = expiry.filter(|&deadline| s.real_weather && deadline > now);
        let end = self.expiry.map_or(now, |deadline| now.min(deadline));
        let weather_or_limit_changed = expiry != self.expiry || s.max_height != self.max_height;
        if rate != self.rate || weather_or_limit_changed {
            for cap in self.caps.values_mut() {
                cap.snow
                    .advance(end, now, self.rate, self.max_height, s.max_height);
            }
        }
        if bottom_rate != self.bottom_rate || weather_or_limit_changed {
            self.bottom_snow
                .advance(end, now, self.bottom_rate, self.max_height, s.max_height);
        }
        // Snow accumulation refills only the grooves rolled out of a cap.
        // Unlike a static subtraction, this also refills when the overall
        // snow depth is already at its height limit.
        if rate > 0.0 {
            for cap in self.caps.values_mut() {
                let elapsed = now.saturating_duration_since(cap.removed_updated).as_secs_f32();
                if elapsed >= 0.35 {
                    let fill = rate * elapsed;
                    let mut changed = false;
                    for removed in &mut cap.removed {
                        if *removed > 0.0 {
                            *removed = (*removed - fill).max(0.0);
                            changed = true;
                        }
                    }
                    if changed {
                        cap.bitmap = None;
                        self.changed = true;
                    }
                    cap.removed_updated = now;
                }
            }
        } else {
            for cap in self.caps.values_mut() {
                cap.removed_updated = now;
            }
        }
        self.rate = rate;
        self.bottom_rate = bottom_rate;
        self.expiry = expiry;
        self.max_height = s.max_height;
        self.advance_ice(now, s, weather.temperature);
        self.update_drips(now, s, weather, paused, sleeping);
        let water_drops: Vec<_> = self.drips.iter().map(|drop| drop.sample(now)).collect();
        let bottom_depth = self
            .bottom_snow
            .height_at(now, self.bottom_rate, self.max_height);
        let bottom_depth = ((bottom_depth * 0.62).min(16.0) * 4.0).floor() / 4.0;
        self.update_shake(now, s, paused || sleeping);
        self.loose_snow
            .retain(|snow| !paused && !sleeping && s.caps && snow.age(now) < SHED_TIME);
        let shedding: Vec<_> = self
            .loose_snow
            .iter()
            .map(|snow| snow.sample(now))
            .collect();
        let density = if s.falling {
            (s.intensity / 100.0 * weather.snow).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut diag = Diagnostics {
            tracked: self.caps.len(),
            snowballs: self.balls.len(),
            snowmen: self.balls.iter().filter(|ball| snowball::is_head_of_snowman(&self.balls, ball.id)).count(),
            reason: "Нет видимых эффектов",
            monitors: self
                .displays
                .iter()
                .map(|d| {
                    (
                        d.name.clone(),
                        format!(
                            "{} × {}  ({}, {})",
                            d.bounds.width(),
                            d.bounds.height(),
                            d.bounds.left,
                            d.bounds.top
                        ),
                    )
                })
                .collect(),
            ..Default::default()
        };
        let mut painted = false;
        let mut fullscreen = false;
        let mut caps_drawn = 0;
        let selected_exists = self
            .displays
            .iter()
            .any(|display| display.name == s.monitor);
        let mut active = Vec::new();
        let mut cap_hits = Vec::new();
        let mut ball_hits = Vec::new();
        if !paused && !sleeping {
            for display in &self.displays {
                if selected_exists && s.monitor != display.name {
                    continue;
                }
                let bounds = display.bounds;
                let covered = s.fullscreen_pause
                    && self.windows.iter()
                        .any(|window| window.visible && window.bounds.covers(bounds));
                if covered {
                    fullscreen = true;
                    continue;
                }
                let count = if density > 0.0 {
                    ((bounds.width() as f32 * bounds.height() as f32 / 7000.0) * density)
                        .ceil()
                        .max(1.0) as usize
                } else {
                    0
                }
                .min(s.particle_limit);
                let falling_clumps: Vec<_> = shedding
                    .iter()
                    .filter(|snow| {
                        snow.x >= (bounds.left - 7) as f32
                            && snow.x < (bounds.right + 7) as f32
                            && snow.y >= (bounds.top - 7) as f32
                            && snow.y < (bounds.bottom + 7) as f32
                    })
                    .collect();
                let has_clumps = !falling_clumps.is_empty();
                let bottom = (s.bottom_snow && bottom_depth >= 1.25).then(|| SnowCap {
                    width: bounds.width() + BOTTOM_INSET * 2,
                    depth: bottom_depth,
                    roughness: s.roughness / 100.0,
                    seed: display.name.bytes().fold(0x7483_00a7u32, |seed, byte| {
                        seed.rotate_left(5) ^ u32::from(byte)
                    }),
                    icicles: false,
                    ice_factor: 0.0,
                    wind_bias: if s.wind_drifts { weather.wind.clamp(-1.0, 1.0) } else { 0.0 },
                });
                let edges: Vec<_> = if s.caps {
                    self.windows
                        .iter()
                        .enumerate()
                        .filter_map(|(index, window)| {
                            if !window.shows_cap(&self.displays) {
                                return None;
                            }
                            let cap = self.caps.get(&window.id)?;
                            let r = window.bounds;
                            let height =
                                (cap.snow.height_at(now, self.rate, self.max_height) * 4.0).floor()
                                    / 4.0;
                            let shape = SnowCap {
                                width: r.width(),
                                depth: height,
                                roughness: s.roughness / 100.0,
                                seed: cap.seed,
                                icicles: s.icicles,
                                ice_factor: (cap.ice_factor * 20.0).round() / 20.0,
                                wind_bias: 0.0,
                            };
                            let rows = shape.rows();
                            (height >= 1.0
                                && r.width() > 20
                                && r.right > bounds.left
                                && r.left < bounds.right
                                && r.top + rows.end > bounds.top
                                && r.top + rows.start < bounds.bottom)
                                .then_some((index, r, shape))
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let balls_here = s.snowballs && self.balls.iter().any(|ball| {
                    let radius = ball.radius();
                    ball.x + radius >= bounds.left as f32
                        && ball.x - radius < bounds.right as f32
                        && ball.y + radius >= bounds.top as f32
                        && ball.y - radius < bounds.bottom as f32
                });
                if count == 0 && edges.is_empty() && !has_clumps && bottom.is_none() && !balls_here {
                    continue;
                }
                let name = &display.name;
                let canvas = match self.surfaces.entry(name.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let Some(canvas) = (unsafe { Canvas::new(self.instance, bounds) }) else {
                            continue;
                        };
                        entry.insert(canvas)
                    }
                };
                active.push(name.clone());
                let cap_changed = canvas.edges != edges || canvas.bottom_shape != bottom;
                if (count != 0 || has_clumps) && animate
                    || cap_changed
                    || canvas.count != count
                    || canvas.shedding != has_clumps
                    || self.changed
                    || count != 0 && !canvas.shown
                {
                    let result = canvas.draw(
                        count,
                        dt,
                        s,
                        weather.wind,
                        &edges,
                        &self.windows,
                        &mut self.caps,
                        bottom,
                        &falling_clumps,
                        &self.balls,
                        &mut self.random,
                        cap_changed || self.changed,
                    );
                    painted |= result;
                    canvas.edges = edges;
                    canvas.bottom_shape = bottom;
                }
                if canvas.shown {
                    diag.particles += count + falling_clumps.len();
                    caps_drawn += canvas.cap_count;
                    diag.bottom_edges += usize::from(bottom.is_some());
                    if s.snowballs {
                        for &(index, rect, shape) in &edges {
                            let window_id = self.windows[index].id;
                            if shape.depth < 2.0 {
                                continue;
                            }
                            let Some(cap) = self.caps.get(&window_id) else {
                                continue;
                            };
                            let heights: Vec<f32> = cap.removed.iter().enumerate().map(|(index, taken)| {
                                let mid = (index * SNOW_SEGMENT + SNOW_SEGMENT / 2)
                                    .min(shape.width.max(1) as usize - 1) as f32;
                                (shape.sample_height(mid) - taken).max(0.0)
                            }).collect();
                            if !heights.iter().any(|&height| height >= 2.0) {
                                continue;
                            }
                            let area = Bounds {
                                top: rect.top - shape.depth.ceil() as i32 - 4,
                                bottom: rect.top,
                                ..rect
                            };
                            let obstacles = self.windows[..index].iter()
                                .filter(|window| window.visible && window.bounds.intersects(area))
                                .map(|window| window.bounds)
                                .collect();
                            cap_hits.push(CapHit {
                                id: window_id,
                                bounds: rect,
                                heights,
                                obstacles,
                            });
                        }
                        for ball in &self.balls {
                            let radius = ball.radius();
                            if ball.x + radius >= bounds.left as f32
                                && ball.x - radius < bounds.right as f32
                                && ball.y + radius >= bounds.top as f32
                                && ball.y - radius < bounds.bottom as f32
                            {
                                ball_hits.push(BallHit {
                                    id: ball.id,
                                    x: ball.x,
                                    y: ball.y,
                                    radius,
                                    decoratable: s.snowman_decorations
                                        && snowball::is_head_of_snowman(&self.balls, ball.id),
                                });
                            }
                        }
                    }
                }
            }
        }
        for (name, canvas) in &mut self.surfaces {
            if !active.contains(name) {
                canvas.hide();
            }
        }
        EVENTS.with_borrow_mut(|events| {
            events.cap_hits = cap_hits;
            events.ball_hits = ball_hits.clone();
            if paused || sleeping || !s.snowballs {
                events.mouse_capturing = false;
                events.mouse_right_capturing = false;
                events.mouse.clear();
            }
        });
        if paused || sleeping || !s.snowballs {
            self.gesture = None;
        }
        diag.moving_balls = !paused && !sleeping && s.snowballs
            && (self.gesture.is_some()
                || self.balls.iter().any(|ball| {
                    ball.parent.is_none() && !ball.grounded
                        && ball_hits.iter().any(|hit| hit.id == ball.id)
                }));
        self.changed = false;
        if painted {
            self.frames += 1;
        }
        let elapsed = now.saturating_duration_since(self.fps_since).as_secs_f32();
        if elapsed >= 1.0 {
            self.fps = (self.frames as f32 / elapsed).round() as u32;
            self.frames = 0;
            self.fps_since = now;
        }
        diag.caps = caps_drawn;
        diag.reason = if sleeping {
            "Экран заблокирован или выключен"
        } else if paused {
            "Эффекты на паузе"
        } else if diag.particles > 0 || diag.caps > 0 || diag.bottom_edges > 0 || !ball_hits.is_empty() {
            "Эффекты включены"
        } else if fullscreen {
            "Полноэкранное приложение"
        } else {
            "Нет видимых эффектов"
        };
        diag.fps = if diag.particles > 0 || diag.caps > 0 || diag.bottom_edges > 0 || !ball_hits.is_empty() {
            self.fps
        } else {
            0
        };
        diag
    }
}

struct Flake {
    x: f32,
    y: f32,
    size: f32,
    phase: f32,
}

type Edge = (usize, Bounds, SnowCap);
const BOTTOM_INSET: i32 = 40;

struct Canvas {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
    pixels: *mut u32,
    bounds: Bounds,
    flakes: Vec<Flake>,
    touched: Vec<usize>,
    cap_pixels: Vec<(usize, u32)>,
    bottom_shape: Option<SnowCap>,
    bottom_bitmap: Option<CapBitmap>,
    edges: Vec<Edge>,
    shown: bool,
    count: usize,
    cap_count: usize,
    shedding: bool,
}

impl Canvas {
    unsafe fn new(instance: HINSTANCE, bounds: Bounds) -> Option<Self> {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            SURFACE,
            w!("Snowy New Year · Snow"),
            WS_POPUP,
            bounds.left,
            bounds.top,
            bounds.width(),
            bounds.height(),
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return None;
        }
        let dc = CreateCompatibleDC(null_mut());
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = bounds.width();
        info.bmiHeader.biHeight = -bounds.height();
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        if bitmap.is_null() || dc.is_null() || pixels.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !dc.is_null() {
                DeleteDC(dc);
            }
            DestroyWindow(hwnd);
            return None;
        }
        let pixels: *mut u32 = pixels.cast();
        pixels.write_bytes(0, bounds.width() as usize * bounds.height() as usize);
        let original = SelectObject(dc, bitmap);
        Some(Self {
            hwnd,
            dc,
            bitmap,
            original,
            pixels,
            bounds,
            flakes: Vec::new(),
            touched: Vec::new(),
            cap_pixels: Vec::new(),
            bottom_shape: None,
            bottom_bitmap: None,
            edges: Vec::new(),
            shown: false,
            count: 0,
            cap_count: 0,
            shedding: false,
        })
    }

    fn hide(&mut self) {
        if self.shown {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
        self.edges.clear();
        self.flakes.clear();
        self.cap_pixels.clear();
        self.bottom_shape = None;
        self.shedding = false;
    }

    fn pixel(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.bounds.width() || y >= self.bounds.height() {
            return;
        }
        let i = (y * self.bounds.width() + x) as usize;
        unsafe {
            let ptr = self.pixels.add(i);
            if *ptr == 0 {
                self.touched.push(i);
            }
            if color >> 24 >= *ptr >> 24 {
                *ptr = color;
            }
        }
    }

    fn draw(
        &mut self,
        count: usize,
        dt: f32,
        s: &Settings,
        wind: f32,
        edges: &[Edge],
        windows: &[Window],
        caps: &mut HashMap<usize, Cap>,
        bottom: Option<SnowCap>,
        falling_clumps: &[&SnowParticle],
        balls: &[Snowball],
        seed: &mut u32,
        rebuild_caps: bool,
    ) -> bool {
        for index in self.touched.drain(..) {
            unsafe {
                *self.pixels.add(index) = 0;
            }
        }
        let bounds = self.bounds;
        let width = bounds.width() as f32;
        let height = bounds.height() as f32;
        if rebuild_caps {
            self.draw_caps(edges, windows, caps);
            if let Some(shape) = bottom {
                self.draw_bottom(shape);
            }
            self.cap_pixels.clear();
            self.cap_pixels.extend(
                self.touched
                    .iter()
                    .map(|&index| (index, unsafe { *self.pixels.add(index) })),
            );
        } else {
            for &(index, pixel) in &self.cap_pixels {
                unsafe {
                    *self.pixels.add(index) = pixel;
                }
                self.touched.push(index);
            }
        }
        if s.snowballs {
            self.draw_balls(balls, s.snowman_decorations);
        }
        self.count = count;
        self.flakes.truncate(count);
        while self.flakes.len() < count {
            self.flakes.push(Flake {
                x: random(seed) * width,
                y: random(seed) * height,
                size: 0.45 + random(seed),
                phase: random(seed) * std::f32::consts::TAU,
            });
        }
        for index in 0..self.flakes.len() {
            let flake = &mut self.flakes[index];
            flake.phase += dt * (0.9 + flake.size);
            flake.x = (flake.x + dt * (wind * 110.0 + flake.phase.sin() * 15.0)).rem_euclid(width);
            flake.y += dt * s.speed * (0.45 + flake.size);
            if flake.y > height + 8.0 {
                flake.y = -8.0;
                flake.x = random(seed) * width;
            }
            let (x, y) = (flake.x, flake.y);
            let radius = (s.size * flake.size * 0.63).max(0.7);
            self.draw_particle(x, y, radius, s.opacity * 2.55);
        }
        self.shedding = !falling_clumps.is_empty();
        for snow in falling_clumps {
            self.draw_particle(
                snow.x - bounds.left as f32,
                snow.y - bounds.top as f32,
                snow.radius,
                snow.opacity,
            );
        }
        if self.touched.is_empty() && count == 0 {
            self.hide();
            return false;
        }
        let destination = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        let origin = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: bounds.width(),
            cy: bounds.height(),
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let result = UpdateLayeredWindow(
                self.hwnd,
                null_mut(),
                &destination,
                &size,
                self.dc,
                &origin,
                0,
                &blend,
                ULW_ALPHA,
            ) != 0;
            if result && !self.shown {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.shown = true;
            }
            result
        }
    }

    fn draw_balls(&mut self, balls: &[Snowball], decorate: bool) {
        let mut sorted: Vec<_> = balls.iter().copied().collect();
        // Bases first; finished snowmen are decorated only after their
        // spheres are composited so hats, faces and scarves stay readable.
        sorted.sort_by(|a, b| b.y.total_cmp(&a.y));
        for ball in sorted {
            let radius = ball.radius();
            if ball.x + radius < self.bounds.left as f32
                || ball.x - radius >= self.bounds.right as f32
                || ball.y + radius < self.bounds.top as f32
                || ball.y - radius >= self.bounds.bottom as f32
            {
                continue;
            }
            self.draw_sphere(ball);
        }
        if decorate {
            for head in balls.iter().copied().filter(|ball| {
                snowball::is_head_of_snowman(balls, ball.id)
            }) {
                if let Some(middle) = head.parent.and_then(|id| {
                    balls.iter().copied().find(|ball| ball.id == id)
                }) {
                    self.draw_snowman_outfit(head, middle);
                }
            }
        }
    }

    fn draw_snowman_outfit(&mut self, head: Snowball, middle: Snowball) {
        let ox = self.bounds.left as f32;
        let oy = self.bounds.top as f32;
        let (hx, hy) = (head.x - ox, head.y - oy);
        let (mx, my) = (middle.x - ox, middle.y - oy);
        let hr = head.radius();
        let mr = middle.radius();
        let (cloth, edge, dark, festive) = match head.outfit % 3 {
            1 => ([192, 52, 70], [252, 239, 223], [65, 43, 57], true),
            2 => ([62, 151, 174], [193, 232, 239], [29, 65, 91], false),
            _ => ([176, 64, 79], [233, 175, 167], [48, 57, 74], false),
        };
        // Twig arms are attached to the middle snowball; they are drawn
        // only for completed snowmen and never interfere with mouse hit tests.
        let branch = [112, 82, 66];
        let left = (mx - mr * 0.74, my - mr * 0.13);
        let right = (mx + mr * 0.74, my - mr * 0.13);
        let left_tip = (mx - mr * 1.65, my - mr * 0.76);
        let right_tip = (mx + mr * 1.65, my - mr * 0.76);
        self.colored_stroke(left, left_tip, (mr * 0.065).max(1.1), branch);
        self.colored_stroke(right, right_tip, (mr * 0.065).max(1.1), branch);
        self.colored_stroke(
            (left_tip.0 + mr * 0.24, left_tip.1 + mr * 0.15),
            (left_tip.0 + mr * 0.11, left_tip.1 - mr * 0.19),
            1.0,
            branch,
        );
        self.colored_stroke(
            (right_tip.0 - mr * 0.24, right_tip.1 + mr * 0.15),
            (right_tip.0 - mr * 0.11, right_tip.1 - mr * 0.19),
            1.0,
            branch,
        );

        // Coal buttons and eyes.
        for offset in [0.0, 0.37, 0.69] {
            self.colored_disc(mx, my + mr * offset, (mr * 0.085).max(1.5), dark);
        }
        for side in [-1.0f32, 1.0] {
            self.colored_disc(
                hx + side * hr * 0.29,
                hy - hr * 0.14,
                (hr * 0.11).max(1.2),
                [45, 53, 67],
            );
        }
        // Slightly elongated carrot, not an oversized round dot.
        self.colored_stroke(
            (hx + hr * 0.06, hy + hr * 0.09),
            (hx + hr * 0.56, hy + hr * 0.24),
            (hr * 0.12).max(1.3),
            [233, 122, 57],
        );

        // Scarf neck with a short hanging tail.
        let neck_y = hy + hr * 0.83;
        self.colored_stroke(
            (hx - hr * 0.83, neck_y),
            (hx + hr * 0.83, neck_y),
            (hr * 0.19).max(1.6),
            cloth,
        );
        self.colored_stroke(
            (hx + hr * 0.45, neck_y),
            (hx + hr * 0.51, neck_y + mr * 0.90),
            (hr * 0.18).max(1.4),
            cloth,
        );
        self.colored_disc(
            hx + hr * 0.51,
            neck_y + mr * 0.90,
            (hr * 0.18).max(1.4),
            edge,
        );

        // Headwear: top hat, Santa hat, knitted winter hat.
        let hat_y = hy - hr * 0.89;
        if festive {
            self.colored_stroke(
                (hx - hr * 0.78, hat_y),
                (hx + hr * 0.78, hat_y),
                (hr * 0.19).max(1.6),
                edge,
            );
            self.colored_stroke(
                (hx - hr * 0.61, hat_y - hr * 0.09),
                (hx + hr * 0.18, hat_y - hr * 0.96),
                (hr * 0.55).max(3.0),
                cloth,
            );
            self.colored_disc(
                hx + hr * 0.18,
                hat_y - hr * 0.98,
                (hr * 0.20).max(2.0),
                edge,
            );
        } else if head.outfit % 3 == 2 {
            self.colored_rounded_cap(
                hx - hr * 0.66,
                hat_y - hr * 0.68,
                hx + hr * 0.66,
                hat_y + hr * 0.06,
                cloth,
            );
            self.colored_stroke(
                (hx - hr * 0.81, hat_y),
                (hx + hr * 0.81, hat_y),
                (hr * 0.15).max(1.5),
                edge,
            );
            self.colored_disc(hx, hat_y - hr * 0.77, (hr * 0.20).max(1.7), edge);
        } else {
            self.colored_rounded_cap(
                hx - hr * 0.53,
                hat_y - hr * 0.85,
                hx + hr * 0.53,
                hat_y + hr * 0.05,
                dark,
            );
            self.colored_stroke(
                (hx - hr * 0.85, hat_y + hr * 0.01),
                (hx + hr * 0.85, hat_y + hr * 0.01),
                (hr * 0.15).max(1.5),
                dark,
            );
            self.colored_stroke(
                (hx - hr * 0.51, hat_y - hr * 0.07),
                (hx + hr * 0.51, hat_y - hr * 0.07),
                (hr * 0.10).max(1.0),
                cloth,
            );
        }
    }

    fn colored_stroke(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        thickness: f32,
        rgb: [u8; 3],
    ) {
        let dx = to.0 - from.0;
        let dy = to.1 - from.1;
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as usize;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            self.colored_disc(from.0 + dx * t, from.1 + dy * t, thickness, rgb);
        }
    }

    fn colored_rounded_cap(
        &mut self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        rgb: [u8; 3],
    ) {
        if right <= left || bottom <= top {
            return;
        }
        for y in top.ceil() as i32..=bottom.floor() as i32 {
            self.colored_stroke(
                (left + 1.0, y as f32),
                (right - 1.0, y as f32),
                1.0,
                rgb,
            );
        }
    }

    fn draw_sphere(&mut self, ball: Snowball) {
        let x = ball.x - self.bounds.left as f32;
        let y = ball.y - self.bounds.top as f32;
        let r = ball.radius();
        let ext = (r + 1.0).ceil() as i32;
        for dy in -ext..=ext {
            for dx in -ext..=ext {
                let (fx, fy) = (dx as f32, dy as f32);
                let dist = fx.hypot(fy);
                let coverage = (r + 0.5 - dist).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let volume = (1.0 - (dist / r).powi(2)).max(0.0).sqrt();
                let highlight = (1.0 - (fx + r * 0.35).hypot(fy + r * 0.40) / (2.0 * r))
                    .clamp(0.0, 1.0);
                let light = (volume * 20.0 + highlight * 17.0 - (fy / r).max(0.0) * 26.0);
                let alpha = (255.0 * coverage).round() as u32;
                let rr = (219.0 + light).clamp(0.0, 255.0) as u32 * alpha / 255;
                let gg = (230.0 + light).clamp(0.0, 255.0) as u32 * alpha / 255;
                let bb = (240.0 + light).clamp(0.0, 255.0) as u32 * alpha / 255;
                self.pixel(x as i32 + dx, y as i32 + dy,
                    (alpha << 24) | (rr << 16) | (gg << 8) | bb);
            }
        }
    }

    fn colored_disc(&mut self, x: f32, y: f32, r: f32, rgb: [u8; 3]) {
        let extent = r.ceil() as i32;
        for dy in -extent..=extent {
            for dx in -extent..=extent {
                let coverage = (r + 0.5 - (dx as f32).hypot(dy as f32)).clamp(0.0, 1.0);
                let alpha = (255.0 * coverage).round() as u32;
                if alpha == 0 { continue; }
                self.pixel(
                    x as i32 + dx, y as i32 + dy,
                    (alpha << 24)
                        | ((rgb[0] as u32 * alpha / 255) << 16)
                        | ((rgb[1] as u32 * alpha / 255) << 8)
                        | (rgb[2] as u32 * alpha / 255),
                );
            }
        }
    }

    fn draw_particle(&mut self, x: f32, y: f32, radius: f32, opacity: f32) {
        let extent = radius.ceil() as i32;
        for dy in -extent..=extent {
            for dx in -extent..=extent {
                let coverage = (radius + 0.4 - (dx as f32).hypot(dy as f32)).clamp(0.0, 1.0);
                let alpha = (coverage * opacity).round() as u32;
                if alpha > 0 {
                    let color = (alpha << 24)
                        | ((alpha * 230 / 255) << 16)
                        | ((alpha * 246 / 255) << 8)
                        | alpha;
                    self.pixel(x as i32 + dx, y as i32 + dy, color);
                }
            }
        }
    }

    fn draw_caps(&mut self, edges: &[Edge], windows: &[Window], caps: &mut HashMap<usize, Cap>) {
        let bounds = self.bounds;
        self.cap_count = 0;
        for &(index, window, shape) in edges.iter().rev() {
            let Some(cap) = caps.get_mut(&windows[index].id) else {
                continue;
            };
            let rows = shape.rows();
            let area = Bounds {
                top: window.top + rows.start,
                bottom: window.top + rows.end,
                ..window
            };
            let obstacles: Vec<_> = windows[..index]
                .iter()
                .filter(|other| other.visible && other.bounds.intersects(area))
                .map(|other| other.bounds)
                .collect();
            let mut drawn = false;
            for &(x, y, pixel) in CapBitmap::pixels(&mut cap.bitmap, shape, &cap.removed) {
                let x = x + window.left;
                let y = y + window.top;
                if bounds.contains(x, y) && !obstacles.iter().any(|other| other.contains(x, y)) {
                    self.pixel(x - bounds.left, y - bounds.top, pixel);
                    drawn = true;
                }
            }
            self.cap_count += usize::from(drawn);
        }
    }

    fn draw_bottom(&mut self, shape: SnowCap) {
        // The profile's baseline sits just outside the last visible screen row.
        let height = self.bounds.height();
        let mut bitmap = self.bottom_bitmap.take();
        for &(x, y, pixel) in CapBitmap::pixels(&mut bitmap, shape, &[]) {
            if y < 0 {
                self.pixel(x - BOTTOM_INSET, height + y, pixel);
            }
        }
        self.bottom_bitmap = bitmap;
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.original);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
            DestroyWindow(self.hwnd);
        }
    }
}
