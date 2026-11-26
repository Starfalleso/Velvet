//! Windows 11 taskbar hiding & work-area management.
//!
//! Combines AppBar auto-hide state management (`SHAppBarMessage(ABM_SETSTATE)`),
//! disabling + hiding `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` so hover cannot pop the
//! taskbar back up, and edge-aware `SPI_SETWORKAREA` expansion that always preserves top-edge
//! AppBars (such as YASB), with guaranteed restoration on exit, session end, or panic.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, GetClassNameW, GetWindowRect,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow,
    SystemParametersInfoW, ASFW_ANY, SPIF_SENDCHANGE, SPI_SETWORKAREA, SWP_HIDEWINDOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNA,
};

#[derive(Default)]
struct TaskbarState {
    changed_autohide: bool,
    original_appbar_state: usize,
    saved_work_areas: Vec<RECT>,
}

static IS_HIDDEN: AtomicBool = AtomicBool::new(false);
static STATE: OnceLock<Mutex<TaskbarState>> = OnceLock::new();

fn state() -> &'static Mutex<TaskbarState> {
    STATE.get_or_init(|| Mutex::new(TaskbarState::default()))
}

fn hidden_flag_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("velvet").join("taskbar-hidden.flag"))
}

fn write_hidden_flag(hidden: bool) {
    let Some(path) = hidden_flag_path() else {
        return;
    };
    if hidden {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, b"1");
    } else {
        let _ = fs::remove_file(path);
    }
}

/// Returns `true` if a previous Velvet process exited uncleanly while hiding the taskbar.
pub fn has_stale_hidden_flag() -> bool {
    hidden_flag_path().map(|p| p.exists()).unwrap_or(false)
}

fn is_explorer_image_path(path: &str) -> bool {
    path.rsplit(['\\', '/'])
        .next()
        .map(|name| name.eq_ignore_ascii_case("explorer.exe"))
        .unwrap_or(false)
}

/// Returns `true` iff `hwnd` belongs to `explorer.exe`.
/// Necessary because third-party bars (such as YASB) can register dummy `0x0` `Shell_TrayWnd`
/// windows that precede Explorer's real `Shell_TrayWnd` in `FindWindowW`.
fn is_explorer_hwnd(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            proc_handle,
            Default::default(),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(proc_handle);
        if !ok || len == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        is_explorer_image_path(&path)
    }
}

/// Finds Explorer's real primary `Shell_TrayWnd` handle, skipping third-party dummy `Shell_TrayWnd`s.
fn find_primary_taskbar() -> Option<HWND> {
    let mut fallback = None;
    unsafe {
        let mut prev = HWND::default();
        loop {
            match FindWindowExW(None, prev, w!("Shell_TrayWnd"), None) {
                Ok(h) if !h.is_invalid() && !h.0.is_null() => {
                    if is_explorer_hwnd(h) {
                        let mut r = RECT::default();
                        let non_empty = GetWindowRect(h, &mut r).is_ok()
                            && (r.right - r.left) > 0
                            && (r.bottom - r.top) > 0;
                        if non_empty {
                            return Some(h);
                        }
                        if fallback.is_none() {
                            fallback = Some(h);
                        }
                    }
                    prev = h;
                }
                _ => break,
            }
        }
    }
    fallback
}

/// Returns `true` when Explorer's `Shell_TrayWnd` exists and its `WS_VISIBLE` flag was cleared via `SW_HIDE`.
/// (Under Windows Auto-Hide, `Shell_TrayWnd` normally remains `WS_VISIBLE` even when collapsed.)
fn is_primary_taskbar_sw_hidden() -> bool {
    find_primary_taskbar()
        .map(|h| unsafe { !IsWindowVisible(h).as_bool() })
        .unwrap_or(false)
}

/// RAII guard that guarantees the Windows taskbar is restored on drop (normal exit or unwinding).
pub struct TaskbarGuard;

impl TaskbarGuard {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for TaskbarGuard {
    fn drop(&mut self) {
        restore_if_hidden();
    }
}

/// Lock-free check suitable for Win32 `SetWinEventHook` callbacks.
pub fn is_hidden() -> bool {
    IS_HIDDEN.load(Ordering::Acquire)
}

/// Returns `true` iff `hwnd` is an Explorer `Shell_TrayWnd` or `Shell_SecondaryTrayWnd`.
pub fn is_taskbar_hwnd(hwnd: HWND) -> bool {
    if hwnd.is_invalid() || hwnd.0.is_null() {
        return false;
    }
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return false;
    }
    let cls = &buf[..len as usize];
    let is_cls = cls.iter().copied().eq("Shell_TrayWnd".encode_utf16())
        || cls
            .iter()
            .copied()
            .eq("Shell_SecondaryTrayWnd".encode_utf16());
    is_cls && is_explorer_hwnd(hwnd)
}

/// Returns the owning Explorer process ID of the real `Shell_TrayWnd`, or `0` if not found.
pub fn primary_taskbar_pid() -> u32 {
    if let Some(primary) = find_primary_taskbar() {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(primary, Some(&mut pid));
        }
        return pid;
    }
    0
}

/// Returns all Explorer primary and secondary taskbar window handles.
fn find_all_taskbars() -> Vec<HWND> {
    let mut hwnds = Vec::new();
    unsafe {
        for class_name in [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
            let mut prev = HWND::default();
            loop {
                match FindWindowExW(None, prev, class_name, None) {
                    Ok(h) if !h.is_invalid() && !h.0.is_null() => {
                        if is_explorer_hwnd(h)
                            && !hwnds.iter().any(|existing: &HWND| existing.0 == h.0)
                        {
                            hwnds.push(h);
                        }
                        prev = h;
                    }
                    _ => break,
                }
            }
        }
    }
    hwnds
}

/// Disables and hides a single taskbar HWND (`EnableWindow(false)` + `SW_HIDE` + `SWP_HIDEWINDOW`)
/// without altering protected shell Z-order. Returns early if already disabled and hidden.
pub fn hide_single_taskbar_hwnd(hwnd: HWND) {
    if !is_hidden() || hwnd.is_invalid() || hwnd.0.is_null() {
        return;
    }
    unsafe {
        let was_visible = IsWindowVisible(hwnd).as_bool();
        let was_enabled = IsWindowEnabled(hwnd).as_bool();
        if !was_visible && !was_enabled {
            return;
        }
        if was_enabled {
            let _ = EnableWindow(hwnd, false);
        }
        if was_visible {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn get_appbar_state(primary: HWND) -> usize {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: primary,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut abd) }
}

fn set_appbar_state(primary: HWND, state_bits: usize) {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: primary,
        lParam: LPARAM(state_bits as isize),
        ..Default::default()
    };
    unsafe {
        let _ = SHAppBarMessage(ABM_SETSTATE, &mut abd);
    }
}

#[derive(Clone, Copy)]
struct MonitorSnapshot {
    hmonitor: HMONITOR,
    rc_monitor: RECT,
    rc_work: RECT,
}

unsafe extern "system" fn enum_monitors_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<MonitorSnapshot>);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmonitor, &mut info).as_bool() {
        list.push(MonitorSnapshot {
            hmonitor,
            rc_monitor: info.rcMonitor,
            rc_work: info.rcWork,
        });
    }
    BOOL(1)
}

fn snapshot_monitors() -> Vec<MonitorSnapshot> {
    let mut list = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitors_proc),
            LPARAM(&mut list as *mut Vec<MonitorSnapshot> as isize),
        );
    }
    list
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Validates that `tray` is a real non-degenerate taskbar rect on `mon` and returns its docked edge.
/// Rejects empty `(0,0,0,0)` / collapsed dummy rects so they can never be misclassified as `Edge::Top`
/// (which would fight top AppBars like YASB via `SPI_SETWORKAREA`).
fn detect_taskbar_edge(tray: RECT, mon: RECT) -> Option<Edge> {
    let tray_w = tray.right - tray.left;
    let tray_h = tray.bottom - tray.top;
    let mon_w = mon.right - mon.left;
    let mon_h = mon.bottom - mon.top;
    if tray_w <= 0 || tray_h <= 0 || mon_w <= 0 || mon_h <= 0 {
        return None;
    }

    if tray_w >= tray_h {
        let near_bottom = (tray.bottom - mon.bottom).abs() <= 12 || tray.top >= mon.top + (mon_h * 2 / 3);
        let near_top = (tray.top - mon.top).abs() <= 4 && tray_h >= 20 && tray.bottom < mon.top + mon_h / 2;
        if near_bottom {
            Some(Edge::Bottom)
        } else if near_top {
            Some(Edge::Top)
        } else {
            None
        }
    } else {
        let near_left = (tray.left - mon.left).abs() <= 4 && tray_w >= 20 && tray.right < mon.left + mon_w / 2;
        let near_right = (tray.right - mon.right).abs() <= 12 || tray.left >= mon.left + (mon_w * 2 / 3);
        if near_left {
            Some(Edge::Left)
        } else if near_right {
            Some(Edge::Right)
        } else {
            None
        }
    }
}

/// Collects taskbar edges before hiding, hides taskbar windows, and reclaims ONLY
/// the edge occupied by each taskbar so top bars like YASB (`rcWork.top`) remain untouched.
fn apply_hide_and_expand_workarea(
    mut saved_originals: Option<&mut Vec<RECT>>,
    update_work_area: bool,
) {
    let taskbars = find_all_taskbars();

    // Collect taskbar (HMONITOR, RECT) pairs before hiding while handles have valid geometry.
    let mut tray_infos: Vec<(HMONITOR, RECT)> = Vec::with_capacity(taskbars.len());
    if update_work_area {
        for hwnd in &taskbars {
            unsafe {
                let mut tr = RECT::default();
                if GetWindowRect(*hwnd, &mut tr).is_ok() {
                    let hmon = MonitorFromWindow(*hwnd, MONITOR_DEFAULTTONEAREST);
                    tray_infos.push((hmon, tr));
                }
            }
        }
    }

    for hwnd in &taskbars {
        hide_single_taskbar_hwnd(*hwnd);
    }

    if !update_work_area {
        return;
    }

    let monitors = snapshot_monitors();
    // Reclaim only the taskbar edge on monitors that still have a gap.
    for mon in &monitors {
        let mut new_work = mon.rc_work;
        let mut modified = false;

        for (hmon, tr) in &tray_infos {
            if *hmon == mon.hmonitor {
                match detect_taskbar_edge(*tr, mon.rc_monitor) {
                    Some(Edge::Bottom) => {
                        if new_work.bottom < mon.rc_monitor.bottom {
                            new_work.bottom = mon.rc_monitor.bottom;
                            modified = true;
                        }
                    }
                    Some(Edge::Top) => {
                        if new_work.top > mon.rc_monitor.top {
                            new_work.top = mon.rc_monitor.top;
                            modified = true;
                        }
                    }
                    Some(Edge::Left) => {
                        if new_work.left > mon.rc_monitor.left {
                            new_work.left = mon.rc_monitor.left;
                            modified = true;
                        }
                    }
                    Some(Edge::Right) => {
                        if new_work.right < mon.rc_monitor.right {
                            new_work.right = mon.rc_monitor.right;
                            modified = true;
                        }
                    }
                    None => {}
                }
            }
        }

        if modified {
            if let Some(saved) = saved_originals.as_deref_mut() {
                if !saved.iter().any(|r| {
                    r.left >= mon.rc_monitor.left
                        && r.right <= mon.rc_monitor.right
                        && r.top >= mon.rc_monitor.top
                        && r.bottom <= mon.rc_monitor.bottom
                }) {
                    saved.push(mon.rc_work);
                }
            }

            unsafe {
                let _ = SystemParametersInfoW(
                    SPI_SETWORKAREA,
                    0,
                    Some(&mut new_work as *mut RECT as *mut _),
                    SPIF_SENDCHANGE,
                );
            }
        }
    }
}

/// Hides the Windows 11 taskbar reliably (auto-hide ON or OFF) while preserving top AppBars.
pub fn hide_taskbar() {
    let Ok(mut guard) = state().lock() else {
        return;
    };

    let first_transition = !is_hidden() && !guard.changed_autohide && guard.saved_work_areas.is_empty();
    IS_HIDDEN.store(true, Ordering::Release);
    write_hidden_flag(true);

    if first_transition {
        if let Some(primary) = find_primary_taskbar() {
            let orig = get_appbar_state(primary);
            let was_autohide = (orig & (ABS_AUTOHIDE as usize)) != 0;
            if !was_autohide {
                guard.original_appbar_state = orig;
                guard.changed_autohide = true;
                set_appbar_state(primary, orig | (ABS_AUTOHIDE as usize));
            }
        }
    }

    apply_hide_and_expand_workarea(Some(&mut guard.saved_work_areas), true);
}

/// Restores the Windows 11 taskbar, original AppBar auto-hide state, and original work areas.
pub fn restore_taskbar() {
    // Clear atomic flag FIRST so any concurrent SetWinEventHook callback becomes a no-op.
    IS_HIDDEN.store(false, Ordering::Release);
    write_hidden_flag(false);

    let (changed_autohide, orig_state, saved) = {
        let Ok(mut guard) = state().lock() else {
            return;
        };
        let ca = guard.changed_autohide;
        let orig = guard.original_appbar_state;
        guard.changed_autohide = false;
        guard.original_appbar_state = 0;
        let rects = std::mem::take(&mut guard.saved_work_areas);
        (ca, orig, rects)
    };

    for hwnd in find_all_taskbars() {
        unsafe {
            if !IsWindowEnabled(hwnd).as_bool() {
                let _ = EnableWindow(hwnd, true);
            }
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }

    if changed_autohide {
        if let Some(primary) = find_primary_taskbar() {
            set_appbar_state(primary, orig_state);
        }
    }

    let is_autohide_now = find_primary_taskbar()
        .map(|h| (get_appbar_state(h) & (ABS_AUTOHIDE as usize)) != 0)
        .unwrap_or(false);

    if !saved.is_empty() {
        for mut rect in saved {
            unsafe {
                let _ = SystemParametersInfoW(
                    SPI_SETWORKAREA,
                    0,
                    Some(&mut rect as *mut RECT as *mut _),
                    SPIF_SENDCHANGE,
                );
            }
        }
    } else if !is_autohide_now {
        // Standalone `--show-taskbar`: only adjust rcWork when Windows taskbar is docked
        // (never shrink rcWork if the user has Windows auto-hide enabled).
        for mon in snapshot_monitors() {
            let mut work = mon.rc_work;
            for hwnd in find_all_taskbars() {
                unsafe {
                    let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    if hmon == mon.hmonitor {
                        let mut tr = RECT::default();
                        if GetWindowRect(hwnd, &mut tr).is_ok() {
                            match detect_taskbar_edge(tr, mon.rc_monitor) {
                                Some(Edge::Bottom) if work.bottom > tr.top && tr.top > mon.rc_monitor.top => {
                                    work.bottom = tr.top;
                                }
                                Some(Edge::Top) if work.top < tr.bottom && tr.bottom < mon.rc_monitor.bottom => {
                                    work.top = tr.bottom;
                                }
                                Some(Edge::Left) if work.left < tr.right && tr.right < mon.rc_monitor.right => {
                                    work.left = tr.right;
                                }
                                Some(Edge::Right) if work.right > tr.left && tr.left > mon.rc_monitor.left => {
                                    work.right = tr.left;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if work != mon.rc_work {
                unsafe {
                    let _ = SystemParametersInfoW(
                        SPI_SETWORKAREA,
                        0,
                        Some(&mut work as *mut RECT as *mut _),
                        SPIF_SENDCHANGE,
                    );
                }
            }
        }
    }

    // If Windows Auto-Hide is active, `SW_SHOWNA` leaves the taskbar collapsed at its offscreen
    // trigger strip. Focusing `Shell_TrayWnd` slides it up into view immediately so Show gives
    // clear visual feedback, and clicking back to any app lets Auto-Hide collapse it normally.
    if is_autohide_now {
        if let Some(primary) = find_primary_taskbar() {
            unsafe {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = ShowWindow(primary, SW_SHOW);
                let _ = SetForegroundWindow(primary);
            }
        }
    }
}

/// Restores the taskbar only if this process previously hid it.
/// Safe to call from panic hooks, WM_ENDSESSION, and Drop guards.
pub fn restore_if_hidden() {
    if is_hidden() {
        restore_taskbar();
    }
}

/// Toggles between hidden and visible taskbar state. Returns the new `is_hidden` state.
/// Also self-heals if `Shell_TrayWnd` was left `SW_HIDE` from an earlier forced termination.
pub fn toggle_taskbar() -> bool {
    if is_hidden() || is_primary_taskbar_sw_hidden() {
        restore_taskbar();
        false
    } else {
        hide_taskbar();
        true
    }
}

/// Periodic watchdog: if taskbar hide mode is enabled and Explorer recreated or
/// re-showed a taskbar window, immediately re-hide it via `SW_HIDE` without calling `SPI_SETWORKAREA`.
pub fn watchdog_tick() {
    if !is_hidden() {
        return;
    }

    let taskbars = find_all_taskbars();
    let any_exposed = taskbars.iter().any(|&hwnd| unsafe {
        IsWindowVisible(hwnd).as_bool() || IsWindowEnabled(hwnd).as_bool()
    });

    if any_exposed {
        apply_hide_and_expand_workarea(None, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_image_path_matches_only_explorer_exe() {
        assert!(is_explorer_image_path(r"C:\Windows\explorer.exe"));
        assert!(is_explorer_image_path(r"C:\WINDOWS\Explorer.EXE"));
        assert!(!is_explorer_image_path(r"C:\Program Files\YASB\yasb.exe"));
        assert!(!is_explorer_image_path(""));
    }

    #[test]
    fn edge_detection_ignores_empty_rects_and_preserves_yasb_top() {
        let mon = RECT {
            left: 0,
            top: 0,
            right: 1536,
            bottom: 864,
        };
        assert_eq!(detect_taskbar_edge(RECT::default(), mon), None);
        assert_eq!(
            detect_taskbar_edge(
                RECT {
                    left: 0,
                    top: 0,
                    right: 1536,
                    bottom: 2,
                },
                mon
            ),
            None
        );
        assert_eq!(
            detect_taskbar_edge(
                RECT {
                    left: 0,
                    top: 862,
                    right: 1536,
                    bottom: 864,
                },
                mon
            ),
            Some(Edge::Bottom)
        );
        assert_eq!(
            detect_taskbar_edge(
                RECT {
                    left: 0,
                    top: 1078,
                    right: 1920,
                    bottom: 1138,
                },
                RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1080,
                }
            ),
            Some(Edge::Bottom)
        );
        assert_eq!(
            detect_taskbar_edge(
                RECT {
                    left: 0,
                    top: 0,
                    right: 1536,
                    bottom: 48,
                },
                mon
            ),
            Some(Edge::Top)
        );
    }
}
