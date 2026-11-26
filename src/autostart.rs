//! Manages the user's Windows Startup shortcut (`Startup\Velvet.lnk`).

use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{S_FALSE, S_OK};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

struct ComGuard {
    should_uninit: bool,
}

impl ComGuard {
    fn init() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self {
            should_uninit: hr == S_OK || hr == S_FALSE,
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn to_wide_null(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Returns `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Velvet.lnk`.
pub fn startup_lnk_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| {
        dir.join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("Velvet.lnk")
    })
}

/// Returns `true` when `Startup\Velvet.lnk` exists.
pub fn is_enabled() -> bool {
    startup_lnk_path().map(|p| p.exists()).unwrap_or(false)
}

/// Creates `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Velvet.lnk`
/// pointing to the current `velvet.exe`.
pub fn enable() -> anyhow::Result<()> {
    let exe_path = std::env::current_exe()?;
    let lnk_path = startup_lnk_path()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve Startup folder path"))?;

    if let Some(parent) = lnk_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let _com = ComGuard::init();
    let exe_w = to_wide_null(exe_path.as_os_str());
    let work_dir_w = exe_path
        .parent()
        .map(|p| to_wide_null(p.as_os_str()));
    let lnk_w = to_wide_null(lnk_path.as_os_str());

    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(exe_w.as_ptr()))?;
        if let Some(ref wd) = work_dir_w {
            link.SetWorkingDirectory(PCWSTR(wd.as_ptr()))?;
        }
        link.SetDescription(w!("Velvet App Launcher"))?;

        let persist: IPersistFile = link.cast()?;
        persist.Save(PCWSTR(lnk_w.as_ptr()), true)?;
    }

    Ok(())
}

/// Removes `Startup\Velvet.lnk` if present.
pub fn disable() -> anyhow::Result<()> {
    let Some(lnk_path) = startup_lnk_path() else {
        return Ok(());
    };
    match fs::remove_file(lnk_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Toggles autostart on/off and returns the resulting enabled state.
pub fn toggle() -> bool {
    if is_enabled() {
        let _ = disable();
        false
    } else {
        let _ = enable();
        is_enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_lnk_path_points_to_startup_folder() {
        let path = startup_lnk_path().expect("startup_lnk_path should resolve on Windows");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("Velvet.lnk"));
        assert_eq!(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("Startup")
        );
    }
}
