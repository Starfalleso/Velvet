//! Win32 window-level helpers for the GPUI popup window:
//! - Strip taskbar/Alt+Tab/GlazeWM footprint (`WS_EX_TOOLWINDOW | WS_EX_TOPMOST`, remove `WS_EX_APPWINDOW`)
//! - Suppress Windows 11 DWM rounded corner & border seam so GPUI's transparent card is crisp
//! - Center on the active monitor (where cursor resides) respecting DPI and `rcWork`
//! - Reliable focus acquisition using `AttachThreadInput` + `SetForegroundWindow`

use std::cell::Cell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallWindowProcW, DefWindowProcW, GetCursorPos, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowThreadProcessId, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_WNDPROC, GWL_EXSTYLE, HWND_TOPMOST, SWP_FRAMECHANGED,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_SETTINGCHANGE,
    WNDPROC, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

/// Transparent shadow padding around the floating card so GPU shadows never clip.
pub const WINDOW_PAD_X: f32 = 24.0;
pub const WINDOW_PAD_TOP: f32 = 18.0;
pub const WINDOW_PAD_BOTTOM: f32 = 36.0;

/// DWMWA_COLOR_NONE constant (0xFFFFFFFE) to suppress Windows 11 DWM 1px border on borderless popups.
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

static ORIG_LAUNCHER_WNDPROC: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static IN_SETTING_CHANGE: Cell<bool> = const { Cell::new(false) };
}

struct SettingChangeResetGuard;

impl Drop for SettingChangeResetGuard {
    fn drop(&mut self) {
        IN_SETTING_CHANGE.with(|c| c.set(false));
    }
}

/// Guards GPUI's `handle_system_settings_changed` against re-entrant `WM_SETTINGCHANGE`
/// delivery while `SHAppBarMessage` pumps messages inside `self.system_settings.borrow_mut()`.
unsafe extern "system" fn launcher_wnd_proc_guard(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let _reset_guard = if msg == WM_SETTINGCHANGE {
        if IN_SETTING_CHANGE.with(|c| c.replace(true)) {
            return LRESULT(0);
        }
        Some(SettingChangeResetGuard)
    } else {
        None
    };

    let raw = ORIG_LAUNCHER_WNDPROC.load(Ordering::Acquire);
    if raw != 0 {
        let orig: WNDPROC = Some(std::mem::transmute::<
            isize,
            unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
        >(raw));
        CallWindowProcW(orig, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Applies Win32 extended styles and DWM attributes to keep the launcher floating,
/// borderless, topmost, and excluded from taskbar / Alt+Tab / GlazeWM tiling.
pub fn configure_launcher_hwnd(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    let hwnd = HWND(hwnd_raw as *mut _);

    unsafe {
        if ORIG_LAUNCHER_WNDPROC.load(Ordering::Acquire) == 0 {
            let guard_ptr = launcher_wnd_proc_guard as *const () as isize;
            let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, guard_ptr);
            if prev != 0 && prev != guard_ptr {
                ORIG_LAUNCHER_WNDPROC.store(prev, Ordering::Release);
            }
        }

        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = (ex_style | (WS_EX_TOOLWINDOW.0 as isize) | (WS_EX_TOPMOST.0 as isize))
            & !(WS_EX_APPWINDOW.0 as isize);
        if new_style != ex_style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        }

        let border_color = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_color as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let corner_pref = DWMWCP_DONOTROUND.0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let nc_policy = DWMNCRP_DISABLED.0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &nc_policy as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

/// Centers the launcher window on the monitor currently containing the cursor,
/// positions it near `top_offset_ratio` of the monitor's `rcWork`, shows it topmost,
/// and reliably transfers keyboard focus.
pub fn center_and_show(
    hwnd_raw: isize,
    logical_width: f32,
    logical_height: f32,
    top_offset_ratio: f32,
) {
    if hwnd_raw == 0 {
        return;
    }
    let hwnd = HWND(hwnd_raw as *mut _);

    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);

        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work: RECT = if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };

        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let total_w = logical_width + 2.0 * WINDOW_PAD_X;
        let total_h = logical_height + WINDOW_PAD_TOP + WINDOW_PAD_BOTTOM;
        let phys_w = (total_w * scale).round() as i32;
        let phys_h = (total_h * scale).round() as i32;

        let work_w = work.right - work.left;
        let work_h = work.bottom - work.top;
        let card_h = (logical_height * scale).round() as i32;

        let x = work.left + ((work_w - phys_w) / 2).max(0);
        let ratio = top_offset_ratio.clamp(0.05, 0.5);
        let card_y = work.top + (((work_h - card_h).max(0) as f32) * ratio).round() as i32;
        let y = (card_y - (WINDOW_PAD_TOP * scale).round() as i32).max(work.top);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            phys_w,
            phys_h,
            SWP_SHOWWINDOW,
        );

        let fg = GetForegroundWindow();
        let our_tid = GetCurrentThreadId();
        let fg_tid = if !fg.is_invalid() && fg.0 != std::ptr::null_mut() {
            GetWindowThreadProcessId(fg, None)
        } else {
            0
        };

        let attached = if fg_tid != 0 && fg_tid != our_tid {
            AttachThreadInput(fg_tid, our_tid, true).as_bool()
        } else {
            false
        };

        let _ = BringWindowToTop(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);

        if attached {
            let _ = AttachThreadInput(fg_tid, our_tid, false);
        }
    }
}

/// Immediately hides the launcher window without destroying its GPUI surface.
pub fn hide_hwnd(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}
