use std::collections::HashSet;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow,
    IsWindowVisible, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use crate::config::{Config, DockSide, WindowMode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    fn from_rect(rect: RECT) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SpotifyWindow {
    pub hwnd: HWND,
    pub rect: RectI,
    pub minimized: bool,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MonitorAreas {
    pub monitor: RectI,
    pub work: RectI,
}

pub fn hwnd_from_raw(handle: raw_window_handle::RawWindowHandle) -> Option<HWND> {
    match handle {
        raw_window_handle::RawWindowHandle::Win32(win32) => {
            Some(HWND(win32.hwnd.get() as *mut core::ffi::c_void))
        }
        _ => None,
    }
}

pub fn is_valid(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

pub fn find_spotify_window(process_name: &str) -> Option<SpotifyWindow> {
    let pids = process_pids(process_name);
    if pids.is_empty() {
        return None;
    }

    let mut ctx = EnumCtx { pids, best: None };
    unsafe {
        let _ = EnumWindows(Some(enum_window), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.best
}

struct EnumCtx {
    pids: HashSet<u32>,
    best: Option<SpotifyWindow>,
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut EnumCtx) };

    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
        return TRUE;
    }
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    if style & WS_EX_TOOLWINDOW.0 as isize != 0 {
        return TRUE;
    }

    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if !ctx.pids.contains(&pid) {
        return TRUE;
    }

    let title_len = unsafe { GetWindowTextLengthW(hwnd) };
    if title_len <= 0 {
        return TRUE;
    }
    let mut title_buf = vec![0u16; title_len as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut title_buf) } as usize;
    title_buf.truncate(copied);
    let title = String::from_utf16_lossy(&title_buf);
    if title.trim().is_empty() {
        return TRUE;
    }

    let minimized = unsafe { IsIconic(hwnd).as_bool() };
    let rect = if minimized {
        RectI::default()
    } else {
        match window_rect(hwnd) {
            Some(rect) if rect.width() >= 200 && rect.height() >= 150 => rect,
            _ => return TRUE,
        }
    };

    let area = rect.width() as i64 * rect.height() as i64;
    let best_area = ctx
        .best
        .as_ref()
        .map(|b| b.rect.width() as i64 * b.rect.height() as i64)
        .unwrap_or(-1);

    if area > best_area {
        ctx.best = Some(SpotifyWindow {
            hwnd,
            rect,
            minimized,
            title,
        });
    }

    TRUE
}

pub fn refresh_spotify(spotify: &SpotifyWindow) -> Option<SpotifyWindow> {
    if !is_valid(spotify.hwnd) {
        return None;
    }
    let minimized = unsafe { IsIconic(spotify.hwnd).as_bool() };
    let rect = if minimized {
        spotify.rect
    } else {
        window_rect(spotify.hwnd)?
    };
    Some(SpotifyWindow {
        hwnd: spotify.hwnd,
        rect,
        minimized,
        title: spotify.title.clone(),
    })
}

fn window_rect(hwnd: HWND) -> Option<RectI> {
    let mut rect = RECT::default();
    let dwm_ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();
    if !dwm_ok && unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return None;
    }
    let out = RectI::from_rect(rect);
    if out.width() <= 0 || out.height() <= 0 {
        None
    } else {
        Some(out)
    }
}

pub fn work_area_for(hwnd: HWND) -> MonitorAreas {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitor_areas(monitor)
}

pub fn primary_work_area() -> MonitorAreas {
    let monitor = unsafe {
        MonitorFromPoint(
            windows::Win32::Foundation::POINT { x: 0, y: 0 },
            MONITOR_DEFAULTTOPRIMARY,
        )
    };
    monitor_areas(monitor)
}

fn monitor_areas(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> MonitorAreas {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        MonitorAreas {
            monitor: RectI::from_rect(info.rcMonitor),
            work: RectI::from_rect(info.rcWork),
        }
    } else {
        MonitorAreas {
            monitor: RectI {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            work: RectI {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
        }
    }
}

pub struct Placement {
    pub rect: RectI,
    pub visible: bool,
}

/// Calcula la posición de la ventana para el modo actual.
pub fn compute_placement(
    config: &Config,
    spotify: Option<&SpotifyWindow>,
    _own_hwnd: HWND,
    pixels_per_point: f32,
) -> Placement {
    let ppp = pixels_per_point.max(0.5);
    let mode = config.mode;

    if mode == WindowMode::Fullscreen {
        let areas = spotify
            .map(|s| work_area_for(s.hwnd))
            .unwrap_or_else(primary_work_area);
        return Placement {
            rect: areas.monitor,
            visible: true,
        };
    }

    let fallback = primary_work_area();
    let areas = spotify.map(|s| work_area_for(s.hwnd)).unwrap_or(fallback);

    if let Some(spotify) = spotify {
        if spotify.minimized && config.window.hide_when_spotify_minimized {
            return Placement {
                rect: RectI::default(),
                visible: false,
            };
        }

        if mode == WindowMode::Overlay {
            return Placement {
                rect: spotify.rect,
                visible: true,
            };
        }

        let spotify_rect = if spotify.minimized {
            centered_spotify_guess(&areas)
        } else {
            spotify.rect
        };

        let dock_width = (config.window.dock_width * ppp).round() as i32;
        let gap = (config.window.dock_gap * ppp).round() as i32;
        let height = spotify_rect.height().max(200);
        let right_space = areas.work.right - (spotify_rect.right + gap);
        let left_space = (spotify_rect.left - gap) - areas.work.left;

        let dock_right = match config.window.dock_side {
            DockSide::Right => true,
            DockSide::Left => false,
            DockSide::Auto => right_space >= dock_width || left_space < dock_width,
        };

        let x = if dock_right {
            spotify_rect.right + gap
        } else {
            spotify_rect.left - gap - dock_width
        };
        let x = if dock_right && right_space < dock_width {
            (areas.work.right - dock_width).max(areas.work.left)
        } else if !dock_right && left_space < dock_width {
            areas.work.left
        } else {
            x
        };

        let y = spotify_rect.top.clamp(
            areas.work.top,
            (areas.work.bottom - height).max(areas.work.top),
        );

        return Placement {
            rect: RectI {
                left: x,
                top: y,
                right: x + dock_width,
                bottom: y + height,
            },
            visible: true,
        };
    }

    // Sin Spotify: el overlay ocupa toda el área de trabajo y el modo acoplado
    // usa la franja derecha como ventana de espera.
    if mode == WindowMode::Overlay {
        return Placement {
            rect: areas.work,
            visible: true,
        };
    }

    let dock_width = (config.window.dock_width * ppp).round() as i32;
    let height = ((areas.work.height() as f32 * 0.8) as i32).max(240);
    let x = areas.work.right - dock_width;
    let y = areas.work.top + (areas.work.height() - height) / 2;
    Placement {
        rect: RectI {
            left: x,
            top: y,
            right: x + dock_width,
            bottom: y + height,
        },
        visible: true,
    }
}

fn centered_spotify_guess(areas: &MonitorAreas) -> RectI {
    let width = areas.work.width() * 3 / 4;
    let height = areas.work.height() * 3 / 4;
    RectI {
        left: areas.work.left + (areas.work.width() - width) / 2,
        top: areas.work.top + (areas.work.height() - height) / 2,
        right: areas.work.left + (areas.work.width() + width) / 2,
        bottom: areas.work.top + (areas.work.height() + height) / 2,
    }
}

pub fn apply_placement(hwnd: HWND, placement: &Placement) {
    if !placement.visible {
        return;
    }
    let rect = placement.rect;
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

pub fn apply_window_styles(hwnd: HWND, overlay: bool, opacity: f32, click_through: bool) {
    unsafe {
        let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut ex = before;
        if overlay {
            ex |= WS_EX_LAYERED.0 as isize;
        } else {
            ex &= !(WS_EX_LAYERED.0 as isize);
        }
        if click_through {
            ex |= WS_EX_TRANSPARENT.0 as isize;
        } else {
            ex &= !(WS_EX_TRANSPARENT.0 as isize);
        }
        if ex != before {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
            log::debug!(
                "Estilos de ventana: 0x{before:X} -> 0x{ex:X} (overlay={overlay}, click={click_through})"
            );
        }
        if overlay {
            let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            let _ = SetLayeredWindowAttributes(
                hwnd,
                windows::Win32::Foundation::COLORREF(0),
                alpha,
                LWA_ALPHA,
            );
        }
    }
}

use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};

pub fn is_minimized(hwnd: HWND) -> bool {
    unsafe { IsIconic(hwnd).as_bool() }
}

pub fn escape_pressed() -> bool {
    unsafe { (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 }
}

pub fn set_dark_title_bar(hwnd: HWND, dark: bool) {
    let dark_mode: u32 = u32::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if dark {
            let caption: u32 = 0x0016_1616;
            let text: u32 = 0x00FF_FFFF;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                &text as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

pub fn outer_rect(hwnd: HWND) -> Option<RectI> {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return None;
    }
    let rect = RectI::from_rect(rect);
    if rect.width() <= 0 || rect.height() <= 0 {
        None
    } else {
        Some(rect)
    }
}

pub fn set_window_position(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn focus_window(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn set_topmost(hwnd: HWND, topmost: bool) {
    let insert_after = if topmost {
        Some(HWND_TOPMOST)
    } else {
        Some(HWND_NOTOPMOST)
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn process_pids(name: &str) -> HashSet<u32> {
    let target = name.trim();
    let target_no_exe = target
        .strip_suffix(".exe")
        .or_else(|| target.strip_suffix(".EXE"))
        .unwrap_or(target);

    let Ok(sys) = sysinfo::System::new_all() else {
        return HashSet::new();
    };
    let mut pids = HashSet::new();
    for (pid, process) in sys.processes() {
        let proc_name = process.name().to_string_lossy();
        if proc_name.eq_ignore_ascii_case(target) || proc_name.eq_ignore_ascii_case(target_no_exe) {
            pids.insert(pid.as_u32());
        }
    }
    pids
}
