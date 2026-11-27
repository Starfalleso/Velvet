use std::sync::Arc;

use crate::config::Config;
use crate::indexer::IndexedApp;
use crate::themes::ThemeChoice;
use crate::updater::UpdateInfo;

/// Commands routed from IPC, global hotkeys, LL keyboard hook, tray menu,
/// config watcher, and background workers into the GPUI main event loop.
#[derive(Debug, Clone)]
pub enum AppCommand {
    Toggle,
    Show,
    Hide,
    ReloadApps,
    AppsIndexed(Arc<Vec<IndexedApp>>),
    ReloadConfig(Box<Config>),
    ApplyTheme(ThemeChoice),
    ToggleWinKeyHook,
    ToggleAutostart,
    ShowTaskbar,
    HideTaskbar,
    ToggleTaskbar,
    TaskbarStateChanged,
    CheckForUpdates { manual: bool, notify_tray: bool },
    UpdateAvailable(UpdateInfo),
    InstallUpdate,
    UpdateStatusHint(String),
    OpenConfig,
    Quit,
}
