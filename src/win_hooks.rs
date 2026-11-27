//! Dedicated Win32 background thread for:
//! - Global hotkey (`RegisterHotKey`, default `Alt+Space`)
//! - Live-toggleable lone `Win` key hook (`WH_KEYBOARD_LL`) that suppresses the Start Menu
//!   on a clean press-and-release while preserving all `Win+*` shortcuts
//! - Global click-outside dismissal (`WH_MOUSE_LL`)
//! - Event-driven `SetWinEventHook` + backup timer watchdog for taskbar-hide mode
//! - System tray icon (`Shell_NotifyIconW`) + native popup menu with Theme submenu,
//!   live `Win`-key toggle, taskbar toggle, and autostart toggle

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use async_channel::Sender;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_NOREPEAT, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowRect, KillTimer, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, SetWindowsHookExW, TrackPopupMenu,
    TranslateMessage, UnhookWindowsHookEx, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, HHOOK,
    HICON, HMENU, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MENU_ITEM_FLAGS, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, MSLLHOOKSTRUCT, OBJID_WINDOW, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, WH_KEYBOARD_LL, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP, WM_NULL,
    WM_QUERYENDSESSION, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use crate::autostart;
use crate::commands::AppCommand;
use crate::config::{parse_hotkey, Config, ThemeConfig};
use crate::icons;
use crate::taskbar;
use crate::themes;
use crate::updater;
use crate::win_window;

const HOTKEY_ID: i32 = 1;
const WATCHDOG_TIMER_ID: usize = 101;
const WATCHDOG_HIDDEN_MS: u32 = 250;
const NIN_BALLOONUSERCLICK: u32 = 0x0405;

const WM_TRAY: u32 = WM_APP + 1;
const WM_SET_WIN_HOOK: u32 = WM_APP + 2;
const WM_SYNC_TASKBAR_WATCH: u32 = WM_APP + 3;
const WM_SET_MOUSE_HOOK: u32 = WM_APP + 4;
const WM_NOTIFY_UPDATE: u32 = WM_APP + 5;

const MENU_TOGGLE: usize = 1001;
const MENU_RELOAD: usize = 1002;
const MENU_TASKBAR: usize = 1003;
const MENU_SETTINGS: usize = 1004;
const MENU_QUIT: usize = 1005;
const MENU_WIN_KEY: usize = 1006;
const MENU_AUTOSTART: usize = 1007;
const MENU_CHECK_UPDATES: usize = 1008;
const MENU_INSTALL_UPDATE: usize = 1009;
const MENU_THEME_BASE: usize = 1010;
const MENU_THEME_MAX: usize = 32;

static HOOK_TX: OnceLock<Sender<AppCommand>> = OnceLock::new();
static TASKBAR_CREATED_MSG: OnceLock<u32> = OnceLock::new();
static TRAY_HICON: OnceLock<isize> = OnceLock::new();

static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static OTHER_KEY_PRESSED: AtomicBool = AtomicBool::new(false);
static WIN_DOWN_MS: AtomicU64 = AtomicU64::new(0);
static MAX_TAP_MS: AtomicU64 = AtomicU64::new(350);
static INTERCEPT_WIN_ENABLED: AtomicBool = AtomicBool::new(false);
static ACTIVE_THEME_SLUG: OnceLock<Mutex<Option<String>>> = OnceLock::new();

static HELPER_HWND: AtomicIsize = AtomicIsize::new(0);
static LAUNCHER_HWND: AtomicIsize = AtomicIsize::new(0);
static LAUNCHER_VISIBLE: AtomicBool = AtomicBool::new(false);
static LAUNCHER_SHOWN_MS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static LL_KBD_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static LL_MOUSE_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static TB_SHOW_HOOK: Cell<Option<HWINEVENTHOOK>> = const { Cell::new(None) };
    static TB_LOC_HOOK: Cell<Option<HWINEVENTHOOK>> = const { Cell::new(None) };
    static TB_WATCH_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static TRAY_THEME_SLUGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn set_launcher_hwnd(hwnd: isize) {
    LAUNCHER_HWND.store(hwnd, Ordering::Relaxed);
}

pub fn set_launcher_visible(visible: bool) {
    if visible {
        LAUNCHER_SHOWN_MS.store(now_ms(), Ordering::Relaxed);
    }
    LAUNCHER_VISIBLE.store(visible, Ordering::Relaxed);
    let helper = HELPER_HWND.load(Ordering::Relaxed);
    if helper != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(helper as *mut _),
                WM_SET_MOUSE_HOOK,
                WPARAM(usize::from(visible)),
                LPARAM(0),
            );
        }
    }
}

pub fn set_max_tap_ms(ms: u64) {
    MAX_TAP_MS.store(ms.max(80), Ordering::Relaxed);
}

pub fn set_active_theme_from_config(theme: &ThemeConfig) {
    let slug = themes::detect_active_theme_slug(theme);
    let lock = ACTIVE_THEME_SLUG.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = slug;
    }
}

#[derive(Clone, Copy)]
pub struct WinHooksHandle {
    helper_hwnd: isize,
}

impl WinHooksHandle {
    pub fn set_intercept_win_key(&self, enabled: bool) {
        if self.helper_hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(self.helper_hwnd as *mut _),
                    WM_SET_WIN_HOOK,
                    WPARAM(usize::from(enabled)),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn sync_taskbar_watch(&self, hidden: bool) {
        if self.helper_hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(self.helper_hwnd as *mut _),
                    WM_SYNC_TASKBAR_WATCH,
                    WPARAM(usize::from(hidden)),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn notify_update(&self, up_to_date_msg: bool) {
        if self.helper_hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(self.helper_hwnd as *mut _),
                    WM_NOTIFY_UPDATE,
                    WPARAM(usize::from(up_to_date_msg)),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn shutdown(&self) {
        if self.helper_hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(self.helper_hwnd as *mut _),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
}

pub fn spawn(config: &Config, tx: Sender<AppCommand>) -> WinHooksHandle {
    let _ = HOOK_TX.set(tx.clone());
    set_max_tap_ms(config.hotkeys.win_key_max_tap_ms);
    set_active_theme_from_config(&config.theme);

    let hotkey_spec = config.hotkeys.toggle.clone();
    let intercept_win = config.hotkeys.intercept_win_key;
    let watchdog_ms = config
        .taskbar
        .watchdog_interval_ms
        .min(WATCHDOG_HIDDEN_MS)
        .max(150);

    let (hwnd_tx, hwnd_rx) = std::sync::mpsc::channel::<isize>();

    thread::Builder::new()
        .name("velvet-win-hooks".to_string())
        .spawn(move || unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = w!("VelvetHelperWnd");

            let wc = WNDCLASSW {
                lpfnWndProc: Some(helper_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let task_created = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = TASKBAR_CREATED_MSG.set(task_created);

            let hinst: HINSTANCE = hinstance.into();
            let hwnd = CreateWindowExW(
                Default::default(),
                class_name,
                w!("VelvetHelper"),
                Default::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(&hinst),
                None,
            )
            .unwrap_or_default();

            let _ = hwnd_tx.send(hwnd.0 as isize);
            if hwnd.is_invalid() || hwnd.0.is_null() {
                return;
            }
            HELPER_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

            // Register global hotkey (default Alt+Space)
            if let Some((mods, vk)) = parse_hotkey(&hotkey_spec) {
                let _ = RegisterHotKey(
                    hwnd,
                    HOTKEY_ID,
                    HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT,
                    vk,
                );
            }

            // Optional lone Win-key hook
            set_win_key_hook_on_thread(intercept_win);

            // Install click-outside mouse hook only while the launcher is actually visible
            if LAUNCHER_VISIBLE.load(Ordering::Relaxed) {
                set_mouse_hook_on_thread(true);
            }

            // Event-driven taskbar watcher only runs when taskbar hiding is active
            if taskbar::is_hidden() {
                set_taskbar_watch_on_thread(hwnd, true, watchdog_ms);
            }

            // Add system tray icon
            if let Some(hicon) = icons::create_tray_hicon() {
                let _ = TRAY_HICON.set(hicon.0 as isize);
                add_tray_icon(hwnd, hicon);
            }

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            HELPER_HWND.store(0, Ordering::Relaxed);
            set_mouse_hook_on_thread(false);
            set_win_key_hook_on_thread(false);
            set_taskbar_watch_on_thread(hwnd, false, watchdog_ms);
            let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
            remove_tray_icon(hwnd);
            if let Some(raw) = TRAY_HICON.get() {
                let _ = DestroyIcon(HICON(*raw as *mut _));
            }
            taskbar::restore_if_hidden();
        })
        .ok();

    let helper_hwnd = hwnd_rx.recv().unwrap_or(0);
    WinHooksHandle { helper_hwnd }
}

unsafe fn set_win_key_hook_on_thread(enable: bool) {
    if enable {
        let already = LL_KBD_HOOK.with(|cell| cell.get().is_some());
        if !already {
            WIN_DOWN.store(false, Ordering::SeqCst);
            OTHER_KEY_PRESSED.store(false, Ordering::SeqCst);
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0).ok();
            let installed = hook.is_some();
            LL_KBD_HOOK.with(|cell| cell.set(hook));
            INTERCEPT_WIN_ENABLED.store(installed, Ordering::Relaxed);
        } else {
            INTERCEPT_WIN_ENABLED.store(true, Ordering::Relaxed);
        }
    } else {
        if let Some(hook) = LL_KBD_HOOK.with(|cell| cell.take()) {
            let _ = UnhookWindowsHookEx(hook);
        }
        WIN_DOWN.store(false, Ordering::SeqCst);
        OTHER_KEY_PRESSED.store(false, Ordering::SeqCst);
        INTERCEPT_WIN_ENABLED.store(false, Ordering::Relaxed);
    }
}

unsafe fn set_mouse_hook_on_thread(enable: bool) {
    if enable {
        let already = LL_MOUSE_HOOK.with(|cell| cell.get().is_some());
        if !already {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse_proc), None, 0).ok();
            LL_MOUSE_HOOK.with(|cell| cell.set(hook));
        }
    } else if let Some(hook) = LL_MOUSE_HOOK.with(|cell| cell.take()) {
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe fn unhook_taskbar_events_on_thread() {
    if let Some(h) = TB_SHOW_HOOK.with(|cell| cell.take()) {
        if !h.is_invalid() {
            let _ = UnhookWinEvent(h);
        }
    }
    if let Some(h) = TB_LOC_HOOK.with(|cell| cell.take()) {
        if !h.is_invalid() {
            let _ = UnhookWinEvent(h);
        }
    }
}

unsafe fn set_taskbar_watch_on_thread(hwnd: HWND, active: bool, watchdog_ms: u32) {
    if active {
        unhook_taskbar_events_on_thread();
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

        let show_hook = SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(tb_win_event_proc),
            0,
            0,
            flags,
        );
        if !show_hook.is_invalid() {
            TB_SHOW_HOOK.with(|cell| cell.set(Some(show_hook)));
        }

        let explorer_pid = taskbar::primary_taskbar_pid();
        if explorer_pid != 0 {
            let loc_hook = SetWinEventHook(
                EVENT_OBJECT_LOCATIONCHANGE,
                EVENT_OBJECT_LOCATIONCHANGE,
                None,
                Some(tb_win_event_proc),
                explorer_pid,
                0,
                flags,
            );
            if !loc_hook.is_invalid() {
                TB_LOC_HOOK.with(|cell| cell.set(Some(loc_hook)));
            }
        }

        let _ = SetTimer(hwnd, WATCHDOG_TIMER_ID, watchdog_ms, None);
        TB_WATCH_ACTIVE.with(|cell| cell.set(true));
    } else {
        unhook_taskbar_events_on_thread();
        if TB_WATCH_ACTIVE.with(|cell| cell.get()) {
            let _ = KillTimer(hwnd, WATCHDOG_TIMER_ID);
            TB_WATCH_ACTIVE.with(|cell| cell.set(false));
        }
    }
}

unsafe extern "system" fn tb_win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if !taskbar::is_hidden() || id_object != OBJID_WINDOW.0 || hwnd.0.is_null() {
        return;
    }
    if taskbar::is_taskbar_hwnd(hwnd) {
        taskbar::hide_single_taskbar_hwnd(hwnd);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

unsafe extern "system" fn ll_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

        // Ignore our own injected dummy mask key (or any injected input)
        if (kb.flags & LLKHF_INJECTED) == Default::default() {
            let vk = kb.vkCode as u16;
            let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
            let msg = wparam.0 as u32;
            let is_down = msg == 0x0100 /* WM_KEYDOWN */ || msg == 0x0104 /* WM_SYSKEYDOWN */;
            let is_up = msg == 0x0101 /* WM_KEYUP */ || msg == 0x0105 /* WM_SYSKEYUP */;

            if is_win {
                if is_down {
                    if !WIN_DOWN.swap(true, Ordering::SeqCst) {
                        OTHER_KEY_PRESSED.store(false, Ordering::SeqCst);
                        WIN_DOWN_MS.store(now_ms(), Ordering::SeqCst);
                    }
                } else if is_up {
                    let was_down = WIN_DOWN.swap(false, Ordering::SeqCst);
                    let other = OTHER_KEY_PRESSED.swap(false, Ordering::SeqCst);
                    let elapsed = now_ms().saturating_sub(WIN_DOWN_MS.load(Ordering::SeqCst));

                    if was_down && !other && elapsed <= MAX_TAP_MS.load(Ordering::Relaxed) {
                        // Inject an unassigned dummy mask key (0xE8) so Windows suppresses
                        // the Start Menu while we still pass the Win keyup through cleanly.
                        inject_dummy_mask_key();
                        if let Some(tx) = HOOK_TX.get() {
                            let _ = tx.try_send(AppCommand::Toggle);
                        }
                    }
                }
            } else if WIN_DOWN.load(Ordering::SeqCst) {
                OTHER_KEY_PRESSED.store(true, Ordering::SeqCst);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn ll_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && LAUNCHER_VISIBLE.load(Ordering::Relaxed) {
        let msg = wparam.0 as u32;
        let is_button_down = matches!(
            msg,
            0x0201 /* WM_LBUTTONDOWN */
                | 0x0204 /* WM_RBUTTONDOWN */
                | 0x0207 /* WM_MBUTTONDOWN */
                | 0x020B /* WM_XBUTTONDOWN */
        );
        if is_button_down && now_ms().saturating_sub(LAUNCHER_SHOWN_MS.load(Ordering::Relaxed)) > 150
        {
            let hwnd_raw = LAUNCHER_HWND.load(Ordering::Relaxed);
            if hwnd_raw != 0 {
                let hwnd = HWND(hwnd_raw as *mut _);
                let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    let dpi = GetDpiForWindow(hwnd);
                    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
                    let card_left = rect.left + (win_window::WINDOW_PAD_X * scale).round() as i32;
                    let card_right = rect.right - (win_window::WINDOW_PAD_X * scale).round() as i32;
                    let card_top = rect.top + (win_window::WINDOW_PAD_TOP * scale).round() as i32;
                    let card_bottom =
                        rect.bottom - (win_window::WINDOW_PAD_BOTTOM * scale).round() as i32;
                    let outside = ms.pt.x < card_left
                        || ms.pt.x > card_right
                        || ms.pt.y < card_top
                        || ms.pt.y > card_bottom;
                    if outside {
                        if let Some(tx) = HOOK_TX.get() {
                            let _ = tx.try_send(AppCommand::Hide);
                        }
                    }
                }
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

unsafe fn inject_dummy_mask_key() {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0xE8),
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0xE8),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

unsafe extern "system" fn helper_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(&tc) = TASKBAR_CREATED_MSG.get() {
        if tc != 0 && msg == tc {
            if let Some(&raw) = TRAY_HICON.get() {
                add_tray_icon(hwnd, HICON(raw as *mut _));
            }
            if taskbar::is_hidden() {
                taskbar::watchdog_tick();
                set_taskbar_watch_on_thread(hwnd, true, WATCHDOG_HIDDEN_MS);
            }
            return LRESULT(0);
        }
    }

    match msg {
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID {
                if let Some(tx) = HOOK_TX.get() {
                    let _ = tx.try_send(AppCommand::Toggle);
                }
            }
            LRESULT(0)
        }
        WM_SET_WIN_HOOK => {
            set_win_key_hook_on_thread(wparam.0 != 0);
            LRESULT(0)
        }
        WM_SYNC_TASKBAR_WATCH => {
            set_taskbar_watch_on_thread(hwnd, wparam.0 != 0, WATCHDOG_HIDDEN_MS);
            LRESULT(0)
        }
        WM_SET_MOUSE_HOOK => {
            set_mouse_hook_on_thread(wparam.0 != 0);
            LRESULT(0)
        }
        WM_NOTIFY_UPDATE => {
            if wparam.0 == 0 {
                if let Some(ver) = updater::latest_version() {
                    show_tray_balloon(
                        hwnd,
                        "Velvet Update Available",
                        &format!("Velvet v{ver} is ready — click here or use the tray menu to update."),
                    );
                }
            } else {
                show_tray_balloon(
                    hwnd,
                    "Velvet",
                    &format!("You're on the latest version (v{}).", updater::current_version()),
                );
            }
            LRESULT(0)
        }
        WM_TRAY => {
            let event = (lparam.0 as u32) & 0xFFFF;
            if event == WM_LBUTTONUP {
                if let Some(tx) = HOOK_TX.get() {
                    let _ = tx.try_send(AppCommand::Toggle);
                }
            } else if event == WM_RBUTTONUP {
                show_tray_menu(hwnd);
            } else if event == NIN_BALLOONUSERCLICK && updater::latest_update().is_some() {
                if let Some(tx) = HOOK_TX.get() {
                    let _ = tx.try_send(AppCommand::InstallUpdate);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = wparam.0 & 0xFFFF;
            if let Some(tx) = HOOK_TX.get() {
                match cmd_id {
                    MENU_TOGGLE => {
                        let _ = tx.try_send(AppCommand::Toggle);
                    }
                    MENU_WIN_KEY => {
                        let _ = tx.try_send(AppCommand::ToggleWinKeyHook);
                    }
                    MENU_TASKBAR => {
                        let _ = tx.try_send(AppCommand::ToggleTaskbar);
                    }
                    MENU_AUTOSTART => {
                        let _ = tx.try_send(AppCommand::ToggleAutostart);
                    }
                    MENU_RELOAD => {
                        let _ = tx.try_send(AppCommand::ReloadApps);
                    }
                    MENU_SETTINGS => {
                        let _ = tx.try_send(AppCommand::OpenConfig);
                    }
                    MENU_CHECK_UPDATES => {
                        let _ = tx.try_send(AppCommand::CheckForUpdates {
                            manual: true,
                            notify_tray: true,
                        });
                    }
                    MENU_INSTALL_UPDATE => {
                        let _ = tx.try_send(AppCommand::InstallUpdate);
                    }
                    cmd_id if (MENU_THEME_BASE..MENU_THEME_BASE + MENU_THEME_MAX).contains(&cmd_id) => {
                        let idx = cmd_id - MENU_THEME_BASE;
                        let slug = TRAY_THEME_SLUGS.with(|cell| cell.borrow().get(idx).cloned());
                        if let Some(choice) = slug.and_then(|s| themes::find_theme(&s)) {
                            let _ = tx.try_send(AppCommand::ApplyTheme(choice));
                        }
                    }
                    MENU_QUIT => {
                        let _ = tx.try_send(AppCommand::Quit);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == WATCHDOG_TIMER_ID {
                taskbar::watchdog_tick();
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => {
            taskbar::restore_if_hidden();
            LRESULT(1)
        }
        WM_ENDSESSION => {
            taskbar::restore_if_hidden();
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            HELPER_HWND.store(0, Ordering::Relaxed);
            set_mouse_hook_on_thread(false);
            set_win_key_hook_on_thread(false);
            set_taskbar_watch_on_thread(hwnd, false, WATCHDOG_HIDDEN_MS);
            taskbar::restore_if_hidden();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn check_flag(checked: bool) -> MENU_ITEM_FLAGS {
    if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING | MF_UNCHECKED
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let Ok(menu): windows::core::Result<HMENU> = CreatePopupMenu() else {
        return;
    };
    let Ok(theme_menu): windows::core::Result<HMENU> = CreatePopupMenu() else {
        let _ = DestroyMenu(menu);
        return;
    };

    let active_slug = ACTIVE_THEME_SLUG
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.clone());
    let mut tray_slugs = Vec::new();
    for (idx, choice) in themes::list_themes().into_iter().take(MENU_THEME_MAX).enumerate() {
        let checked = active_slug.as_deref() == Some(choice.slug());
        let label_w: Vec<u16> = OsStr::new(choice.name())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = AppendMenuW(
            theme_menu,
            check_flag(checked),
            MENU_THEME_BASE + idx,
            PCWSTR(label_w.as_ptr()),
        );
        tray_slugs.push(choice.slug().to_string());
    }
    TRAY_THEME_SLUGS.with(|cell| {
        *cell.borrow_mut() = tray_slugs;
    });

    let _ = AppendMenuW(menu, MF_STRING, MENU_TOGGLE, w!("Toggle Velvet"));
    if let Some(ver) = updater::latest_version() {
        let label_w: Vec<u16> = OsStr::new(&format!("Update Available (v{ver})"))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = AppendMenuW(menu, MF_STRING, MENU_INSTALL_UPDATE, PCWSTR(label_w.as_ptr()));
    }
    let _ = AppendMenuW(menu, MF_POPUP, theme_menu.0 as usize, w!("Theme"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(
        menu,
        check_flag(INTERCEPT_WIN_ENABLED.load(Ordering::Relaxed)),
        MENU_WIN_KEY,
        w!("Intercept Windows Key"),
    );
    let _ = AppendMenuW(
        menu,
        check_flag(taskbar::is_hidden()),
        MENU_TASKBAR,
        w!("Hide Windows Taskbar"),
    );
    let _ = AppendMenuW(
        menu,
        check_flag(autostart::is_enabled()),
        MENU_AUTOSTART,
        w!("Start with Windows"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(menu, MF_STRING, MENU_RELOAD, w!("Reload Apps"));
    let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, w!("Open Config"));
    if !updater::is_packaged_msix() && updater::latest_version().is_none() {
        let _ = AppendMenuW(menu, MF_STRING, MENU_CHECK_UPDATES, w!("Check for Updates"));
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit Velvet"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
}

unsafe fn add_tray_icon(hwnd: HWND, hicon: HICON) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        ..Default::default()
    };

    let tip: Vec<u16> = OsStr::new("Velvet").encode_wide().collect();
    let len = tip.len().min(nid.szTip.len() - 1);
    nid.szTip[..len].copy_from_slice(&tip[..len]);

    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn show_tray_balloon(hwnd: HWND, title: &str, body: &str) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };

    let title_w: Vec<u16> = OsStr::new(title).encode_wide().collect();
    let tlen = title_w.len().min(nid.szInfoTitle.len() - 1);
    nid.szInfoTitle[..tlen].copy_from_slice(&title_w[..tlen]);

    let body_w: Vec<u16> = OsStr::new(body).encode_wide().collect();
    let blen = body_w.len().min(nid.szInfo.len() - 1);
    nid.szInfo[..blen].copy_from_slice(&body_w[..blen]);

    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}
