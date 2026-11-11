use eframe::egui::{self, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows::{
    core::Interface,
    System::{DispatcherQueue, DispatcherQueueController},
    Win32::System::WinRT::{
        Composition::ICompositorDesktopInterop, CreateDispatcherQueueController,
        DispatcherQueueOptions, RoInitialize, RoUninitialize, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
        RO_INIT_SINGLETHREADED,
    },
    UI::Composition::{
        CompositionBackdropBrush, Compositor, ContainerVisual, Desktop::DesktopWindowTarget,
    },
};
use windows_numerics::{Vector2, Vector3};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::ClientToScreen},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

#[derive(Clone, Copy, PartialEq)]
struct GlassArea {
    rect: Rect,
    clip: Rect,
    radius: u8,
}

thread_local! {
    static AREAS: RefCell<Vec<GlassArea>> = const { RefCell::new(Vec::new()) };
    static NATIVE: Cell<bool> = const { Cell::new(false) };
}

pub fn record(rect: Rect, clip: Rect, radius: u8) {
    if has_material() && rect.intersects(clip) {
        AREAS.with_borrow_mut(|areas| areas.push(GlassArea { rect, clip, radius }));
    }
}

pub fn begin_frame(native: bool) {
    NATIVE.set(native);
    AREAS.with_borrow_mut(Vec::clear);
}

pub fn has_material() -> bool {
    NATIVE.get()
}

pub struct Backdrop {
    owner: HWND,
    window: HWND,
    composition: GlassComposition,
    last_areas: Vec<GlassArea>,
    last_scale: f32,
}

impl Backdrop {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let RawWindowHandle::Win32(handle) = cc.window_handle().ok()?.as_raw() else {
            return None;
        };
        let owner = handle.hwnd.get() as HWND;
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = w!("SnowyNewYearGlass");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(backdrop_proc),
                hInstance: instance,
                lpszClassName: class,
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                return None;
            }
            let window = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
                class,
                null(),
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
            if window.is_null() {
                return None;
            }
            let composition = match GlassComposition::new(window) {
                Ok(composition) => composition,
                Err(error) => {
                    crate::settings::log(&format!("Glass backdrop: {error}"));
                    DestroyWindow(window);
                    return None;
                }
            };
            if SetWindowSubclass(owner, Some(owner_proc), 1, window as usize) == 0 {
                DestroyWindow(window);
                return None;
            }
            Some(Self {
                owner,
                window,
                composition,
                last_areas: Vec::new(),
                last_scale: 0.0,
            })
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        let scale = ctx.pixels_per_point();
        let changed =
            AREAS.with_borrow(|areas| *areas != self.last_areas || scale != self.last_scale);
        unsafe {
            if changed {
                if let Err(error) = AREAS.with_borrow(|areas| self.composition.update(areas, scale))
                {
                    crate::settings::log(&format!("Glass surfaces: {error}"));
                    self.hide();
                    return;
                }
                AREAS.with_borrow(|areas| self.last_areas.clone_from(areas));
                self.last_scale = scale;
            }
            if changed || IsWindowVisible(self.window) == 0 {
                move_below(self.owner, self.window, !self.last_areas.is_empty());
            }
        }
    }

    pub fn hide(&self) {
        unsafe {
            ShowWindow(self.window, SW_HIDE);
        }
    }
}

struct WinRtApartment;

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

struct GlassComposition {
    compositor: Compositor,
    root: ContainerVisual,
    brush: CompositionBackdropBrush,
    _target: DesktopWindowTarget,
    _queue: Option<DispatcherQueueController>,
    _apartment: WinRtApartment,
}

impl GlassComposition {
    unsafe fn new(window: HWND) -> windows::core::Result<Self> {
        let enabled = 1i32;
        windows::core::HRESULT(DwmSetWindowAttribute(
            window,
            DWMWA_USE_HOSTBACKDROPBRUSH as u32,
            (&enabled as *const i32).cast(),
            4,
        ))
        .ok()?;
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        let apartment = WinRtApartment;
        let queue = if DispatcherQueue::GetForCurrentThread().is_ok() {
            None
        } else {
            Some(CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?)
        };
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target =
            interop.CreateDesktopWindowTarget(windows::Win32::Foundation::HWND(window), false)?;
        let root = compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let brush = compositor.CreateHostBackdropBrush()?;
        Ok(Self {
            compositor,
            root,
            brush,
            _target: target,
            _queue: queue,
            _apartment: apartment,
        })
    }

    fn update(&self, areas: &[GlassArea], scale: f32) -> windows::core::Result<()> {
        let collection = self.root.Children()?;
        collection.RemoveAll()?;
        for area in areas {
            let rect = area.rect.shrink(1.0).intersect(area.clip);
            if !rect.is_positive() {
                continue;
            }
            let offset = Vector3 {
                X: rect.left() * scale,
                Y: rect.top() * scale,
                Z: 0.0,
            };
            let size = Vector2 {
                X: rect.width() * scale,
                Y: rect.height() * scale,
            };
            let radius = ((f32::from(area.radius) - 1.0) * scale)
                .min(size.X / 2.0)
                .min(size.Y / 2.0);
            let rounded = self.compositor.CreateRoundedRectangleGeometry()?;
            rounded.SetSize(size)?;
            rounded.SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })?;
            let clip = self.compositor.CreateGeometricClipWithGeometry(&rounded)?;
            let visual = self.compositor.CreateSpriteVisual()?;
            visual.SetOffset(offset)?;
            visual.SetSize(size)?;
            visual.SetClip(&clip)?;
            visual.SetBrush(&self.brush)?;
            collection.InsertAtTop(&visual)?;
        }
        Ok(())
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.owner, Some(owner_proc), 1);
            DestroyWindow(self.window);
        }
    }
}

unsafe fn move_below(owner: HWND, window: HWND, show: bool) {
    if !show || IsWindowVisible(owner) == 0 || IsIconic(owner) != 0 {
        ShowWindow(window, SW_HIDE);
        return;
    }
    let mut client: RECT = zeroed();
    let mut origin = POINT { x: 0, y: 0 };
    if GetClientRect(owner, &mut client) != 0 && ClientToScreen(owner, &mut origin) != 0 {
        SetWindowPos(
            window,
            owner,
            origin.x,
            origin.y,
            client.right,
            client.bottom,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let result = DefSubclassProc(hwnd, msg, wp, lp);
    let window = data as HWND;
    match msg {
        WM_NCDESTROY => {
            ShowWindow(window, SW_HIDE);
        }
        WM_WINDOWPOSCHANGED | WM_SHOWWINDOW | WM_ACTIVATE => {
            move_below(hwnd, window, IsWindowVisible(window) != 0);
        }
        _ => {}
    }
    result
}

unsafe extern "system" fn backdrop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
