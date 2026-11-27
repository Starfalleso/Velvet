//! App indexer for Windows (`shell:AppsFolder` UWP/desktop apps + Start Menu `.lnk`/`.exe`),
//! built-in system/power actions, and launch helpers.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use gpui::RenderImage;
use walkdir::WalkDir;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::WaitForMultipleObjects;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
    ShellExecuteW, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::commands::AppCommand;
use crate::config::SearchConfig;
use crate::icons;
use crate::themes::{self, ThemeChoice};
use crate::updater;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    App,
    Store,
}

#[derive(Debug, Clone)]
pub enum LaunchTarget {
    AppsFolder(String),
    Path(PathBuf),
}

#[derive(Clone)]
pub struct IndexedApp {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub category: AppCategory,
    pub launch: LaunchTarget,
    pub keywords: String,
    pub icon: Option<Arc<RenderImage>>,
}

impl std::fmt::Debug for IndexedApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexedApp")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("category", &self.category)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemActionKind {
    Lock,
    Sleep,
    Restart,
    Shutdown,
    ReloadApps,
    ToggleTaskbar,
    ToggleWinKeyHook,
    OpenConfig,
    CheckUpdates,
    InstallUpdate,
    SetTheme(ThemeChoice),
}

#[derive(Debug, Clone)]
pub struct SystemAction {
    pub kind: SystemActionKind,
    pub name: String,
    pub keywords: String,
    pub is_power: bool,
}

pub fn builtin_system_actions(taskbar_hidden: bool, intercept_win_key: bool) -> Vec<SystemAction> {
    let mut actions = vec![
        SystemAction {
            kind: SystemActionKind::ToggleTaskbar,
            name: if taskbar_hidden {
                "Show Windows Taskbar".to_string()
            } else {
                "Hide Windows Taskbar".to_string()
            },
            keywords: "taskbar hide show toggle shell_traywnd bar".to_string(),
            is_power: false,
        },
        SystemAction {
            kind: SystemActionKind::ToggleWinKeyHook,
            name: if intercept_win_key {
                "Disable Windows Key Hook".to_string()
            } else {
                "Enable Windows Key Hook".to_string()
            },
            keywords: "windows key win hook intercept start menu hotkey".to_string(),
            is_power: false,
        },
        SystemAction {
            kind: SystemActionKind::ReloadApps,
            name: "Reload App Index".to_string(),
            keywords: "reload refresh index rescan apps".to_string(),
            is_power: false,
        },
        SystemAction {
            kind: SystemActionKind::OpenConfig,
            name: "Open Launcher Settings".to_string(),
            keywords: "settings config preferences theme toml".to_string(),
            is_power: false,
        },
        if let Some(ver) = updater::latest_version() {
            SystemAction {
                kind: SystemActionKind::InstallUpdate,
                name: format!("Update Velvet to v{ver}"),
                keywords: "update upgrade new version release install download".to_string(),
                is_power: false,
            }
        } else {
            SystemAction {
                kind: SystemActionKind::CheckUpdates,
                name: "Check for Updates".to_string(),
                keywords: "update upgrade version github release check".to_string(),
                is_power: false,
            }
        },
        SystemAction {
            kind: SystemActionKind::Lock,
            name: "Lock Screen".to_string(),
            keywords: "lock screen workstation security".to_string(),
            is_power: true,
        },
        SystemAction {
            kind: SystemActionKind::Sleep,
            name: "Sleep".to_string(),
            keywords: "sleep suspend power".to_string(),
            is_power: true,
        },
        SystemAction {
            kind: SystemActionKind::Restart,
            name: "Restart".to_string(),
            keywords: "restart reboot power".to_string(),
            is_power: true,
        },
        SystemAction {
            kind: SystemActionKind::Shutdown,
            name: "Shut Down".to_string(),
            keywords: "shutdown shut down power off".to_string(),
            is_power: true,
        },
    ];

    for choice in themes::list_themes() {
        let name = format!("Theme: {}", choice.name());
        let keywords = choice.keywords();
        actions.push(SystemAction {
            kind: SystemActionKind::SetTheme(choice),
            name,
            keywords,
            is_power: false,
        });
    }

    actions
}

struct ComGuard;

impl ComGuard {
    fn init() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Self
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
pub fn index_apps_fast(cfg: &SearchConfig) -> Vec<IndexedApp> {
    let _com = ComGuard::init();
    let mut entries = collect_entries(cfg);
    entries.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()));
    entries.into_iter().map(|(app, _)| app).collect()
}

/// Streams app indexing in two phases on the COM thread:
/// 1. Fast metadata pass (< 40 ms) so apps are immediately searchable.
/// 2. Icon extraction pass via `IShellItemImageFactory`.
pub fn index_all_apps_streaming<F>(cfg: &SearchConfig, mut on_batch: F)
where
    F: FnMut(Vec<IndexedApp>),
{
    let _com = ComGuard::init();
    let mut entries = collect_entries(cfg);
    entries.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()));

    let fast_apps: Vec<IndexedApp> = entries.iter().map(|(app, _)| app.clone()).collect();
    on_batch(fast_apps);

    let mut enriched = Vec::with_capacity(entries.len());
    for (mut app, shell_item) in entries {
        app.icon = match (&shell_item, &app.launch) {
            (Some(item), _) => icons::extract_shell_item_icon(item, 48),
            (None, LaunchTarget::Path(path)) => icons::extract_path_icon(path, 48),
            _ => None,
        };
        enriched.push(app);
    }
    on_batch(enriched);
}

fn collect_entries(cfg: &SearchConfig) -> Vec<(IndexedApp, Option<IShellItem>)> {
    let mut entries = Vec::new();
    let mut seen_names = HashSet::new();

    if cfg.index_uwp {
        index_apps_folder(&mut entries, &mut seen_names);
    }
    if cfg.index_start_menu {
        index_start_menu(&mut entries, &mut seen_names);
    }
    entries
}

fn index_apps_folder(
    entries: &mut Vec<(IndexedApp, Option<IShellItem>)>,
    seen_names: &mut HashSet<String>,
) {
    unsafe {
        let Ok(folder): windows::core::Result<IShellItem> =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
        else {
            return;
        };

        let Ok(enumerator): windows::core::Result<IEnumShellItems> =
            folder.BindToHandler(None, &BHID_EnumItems)
        else {
            return;
        };

        loop {
            let mut items: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            let hr = enumerator.Next(&mut items, Some(&mut fetched));
            if hr.is_err() || fetched == 0 {
                break;
            }
            let Some(item) = items[0].take() else {
                continue;
            };

            let Some(name) = shell_item_display_name(&item, SIGDN_NORMALDISPLAY) else {
                continue;
            };
            let name_trimmed = name.trim().to_string();
            if name_trimmed.is_empty() || is_noise_name(&name_trimmed) {
                continue;
            }

            let Some(aumid) = shell_item_display_name(&item, SIGDN_PARENTRELATIVEPARSING) else {
                continue;
            };

            let key = normalize_name(&name_trimmed);
            if !seen_names.insert(key) {
                continue;
            }

            let is_store = aumid.contains('!');
            let category = if is_store {
                AppCategory::Store
            } else {
                AppCategory::App
            };
            let keywords = format!("{} {}", name_trimmed, aumid);

            entries.push((
                IndexedApp {
                    id: format!("aumid:{aumid}"),
                    name: name_trimmed,
                    subtitle: String::new(),
                    category,
                    launch: LaunchTarget::AppsFolder(aumid),
                    keywords,
                    icon: None,
                },
                Some(item),
            ));
        }
    }
}

fn index_start_menu(
    entries: &mut Vec<(IndexedApp, Option<IShellItem>)>,
    seen_names: &mut HashSet<String>,
) {
    let mut roots = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase());
            if !matches!(ext.as_deref(), Some("lnk") | Some("exe")) {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let name = stem.trim().to_string();
            if name.is_empty() || is_noise_name(&name) {
                continue;
            }

            let key = normalize_name(&name);
            if !seen_names.insert(key) {
                continue;
            }

            let folder_hint = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .filter(|s| !s.eq_ignore_ascii_case("Programs"))
                .unwrap_or("");

            let keywords = format!("{name} {folder_hint}");
            let id = format!("path:{}", path.to_string_lossy().to_lowercase());

            entries.push((
                IndexedApp {
                    id,
                    name,
                    subtitle: folder_hint.to_string(),
                    category: AppCategory::App,
                    launch: LaunchTarget::Path(path.to_path_buf()),
                    keywords,
                    icon: None,
                },
                None,
            ));
        }
    }
}

unsafe fn shell_item_display_name(
    item: &IShellItem,
    sigdn: windows::Win32::UI::Shell::SIGDN,
) -> Option<String> {
    let pwstr: PWSTR = item.GetDisplayName(sigdn).ok()?;
    if pwstr.is_null() {
        return None;
    }
    let s = pwstr.to_string().ok();
    CoTaskMemFree(Some(pwstr.0 as *const _));
    s
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn is_noise_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("uninstall ")
        || lower == "uninstall"
        || lower.contains("uninstall")
        || lower.starts_with("readme")
        || lower.ends_with(" help")
        || lower == "help"
        || lower.contains("release notes")
        || lower.contains("documentation")
}

/// Launches an indexed app on a detached background call.
/// For desktop `Path` targets, `as_admin = true` uses the `"runas"` UAC elevation verb.
pub fn launch_app(target: &LaunchTarget, as_admin: bool) {
    match target {
        LaunchTarget::AppsFolder(aumid) => {
            let arg = format!(r"shell:AppsFolder\{aumid}");
            let arg_w: Vec<u16> = OsStr::new(&arg).encode_wide().chain(Some(0)).collect();
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    w!("open"),
                    w!("explorer.exe"),
                    PCWSTR(arg_w.as_ptr()),
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
        LaunchTarget::Path(path) => {
            let path_w: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
            let dir_w: Option<Vec<u16>> = path
                .parent()
                .map(|p| OsStr::new(p).encode_wide().chain(Some(0)).collect());
            let dir_pcw = dir_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null());
            let verb = if as_admin { w!("runas") } else { w!("open") };
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    verb,
                    PCWSTR(path_w.as_ptr()),
                    None,
                    dir_pcw,
                    SW_SHOWNORMAL,
                );
            }
        }
    }
}

fn split_command_and_args(cmd: &str) -> (&str, &str) {
    let trimmed = cmd.trim();
    if let Some(after_quote) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = after_quote.find('"') {
            let exe = &after_quote[..end_quote];
            let rest = after_quote[end_quote + 1..].trim();
            return (exe, rest);
        }
    }
    match trimmed.find(char::is_whitespace) {
        Some(idx) => (&trimmed[..idx], trimmed[idx..].trim()),
        None => (trimmed, ""),
    }
}

/// Executes a `> ...` command line. Tries `ShellExecuteW` directly first (zero cmd flash for
/// GUI tools, URLs, and Control Panel applets), falling back to `cmd.exe /K` for shell built-ins.
pub fn run_shell_command(cmd_line: &str, as_admin: bool) {
    let trimmed = cmd_line.trim();
    if trimmed.is_empty() {
        return;
    }
    let verb = if as_admin { w!("runas") } else { w!("open") };
    let (program, args) = split_command_and_args(trimmed);
    let prog_w: Vec<u16> = OsStr::new(program).encode_wide().chain(Some(0)).collect();
    let args_w: Option<Vec<u16>> = (!args.is_empty())
        .then(|| OsStr::new(args).encode_wide().chain(Some(0)).collect());
    let args_pcw = args_w
        .as_ref()
        .map(|v| PCWSTR(v.as_ptr()))
        .unwrap_or(PCWSTR::null());

    let res = unsafe {
        ShellExecuteW(
            None,
            verb,
            PCWSTR(prog_w.as_ptr()),
            args_pcw,
            None,
            SW_SHOWNORMAL,
        )
    };
    if (res.0 as isize) <= 32 {
        let cmd_args = format!("/K {trimmed}");
        let cmd_args_w: Vec<u16> = OsStr::new(&cmd_args).encode_wide().chain(Some(0)).collect();
        unsafe {
            let _ = ShellExecuteW(
                None,
                verb,
                w!("cmd.exe"),
                PCWSTR(cmd_args_w.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

/// Watches Start Menu Programs folders via OS kernel notifications (`FindFirstChangeNotificationW`)
/// at 0% idle CPU and silently refreshes the app index when shortcuts/exes are added or removed.
pub fn spawn_start_menu_watcher(
    tx: async_channel::Sender<AppCommand>,
    search_cfg: SearchConfig,
) {
    std::thread::Builder::new()
        .name("velvet-startmenu-watch".to_string())
        .spawn(move || {
            let mut dirs = Vec::new();
            if let Ok(pd) = std::env::var("ProgramData") {
                let p = PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs");
                if p.exists() {
                    dirs.push(p);
                }
            }
            if let Ok(ad) = std::env::var("APPDATA") {
                let p = PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs");
                if p.exists() {
                    dirs.push(p);
                }
            }
            if dirs.is_empty() {
                return;
            }

            let flags = FILE_NOTIFY_CHANGE_FILE_NAME
                | FILE_NOTIFY_CHANGE_DIR_NAME
                | FILE_NOTIFY_CHANGE_LAST_WRITE;
            let mut handles: Vec<HANDLE> = Vec::with_capacity(dirs.len());
            for dir in dirs {
                let wide: Vec<u16> = OsStr::new(&dir).encode_wide().chain(Some(0)).collect();
                if let Ok(h) = unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), true, flags) }
                {
                    if !h.is_invalid() && !h.0.is_null() {
                        handles.push(h);
                    }
                }
            }
            if handles.is_empty() {
                return;
            }

            loop {
                let wait = unsafe { WaitForMultipleObjects(&handles, false, u32::MAX) };
                let idx = wait.0.saturating_sub(WAIT_OBJECT_0.0) as usize;
                if idx >= handles.len() {
                    break;
                }
                let _ = unsafe { FindNextChangeNotification(handles[idx]) };

                // Coalesce multi-file installer writes over a 2s quiet window (max ~20s).
                for _ in 0..10 {
                    let deb = unsafe { WaitForMultipleObjects(&handles, false, 2000) };
                    if deb == WAIT_TIMEOUT {
                        break;
                    }
                    let d_idx = deb.0.saturating_sub(WAIT_OBJECT_0.0) as usize;
                    if d_idx < handles.len() {
                        let _ = unsafe { FindNextChangeNotification(handles[d_idx]) };
                    } else {
                        break;
                    }
                }

                index_all_apps_streaming(&search_cfg, |apps| {
                    let _ = tx.send_blocking(AppCommand::AppsIndexed(Arc::new(apps)));
                });
            }
        })
        .ok();
}

/// Opens a file (e.g. `config.toml`) in the user's default editor.
pub fn open_file_in_shell(path: &Path) {
    let path_w: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path_w.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Executes a Windows power/session action.
pub fn execute_power_action(kind: SystemActionKind) {
    match kind {
        SystemActionKind::Lock => unsafe {
            let _ = LockWorkStation();
        },
        SystemActionKind::Sleep => unsafe {
            let _ = SetSuspendState(false, false, false);
        },
        SystemActionKind::Restart => {
            let _ = Command::new("shutdown.exe")
                .args(["/r", "/t", "0"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
        SystemActionKind::Shutdown => {
            let _ = Command::new("shutdown.exe")
                .args(["/s", "/t", "0"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_indexer_finds_windows_apps() {
        let cfg = SearchConfig::default();
        let apps = index_apps_fast(&cfg);
        assert!(
            !apps.is_empty(),
            "expected fast indexer to discover installed Windows apps"
        );
    }
}
