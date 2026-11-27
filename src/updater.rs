//! Lightweight GitHub Release update checker & one-click updater using WinHTTP.
//! Automatically disables itself when running inside a Microsoft Store / MSIX package.

use std::ffi::{c_void, OsStr};
use std::fs::{self, File};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context};
use serde::Deserialize;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Starfalleso/Velvet/releases/latest";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static LATEST_UPDATE: OnceLock<Mutex<Option<UpdateInfo>>> = OnceLock::new();

pub fn set_latest_update(info: Option<UpdateInfo>) {
    let lock = LATEST_UPDATE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = info;
    }
}

pub fn latest_update() -> Option<UpdateInfo> {
    LATEST_UPDATE
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.clone())
}

pub fn latest_version() -> Option<String> {
    latest_update().map(|u| u.version)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub html_url: String,
    pub installer_url: Option<String>,
    pub installer_name: Option<String>,
    pub portable_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateApplyResult {
    LaunchedInstallerQuitNeeded,
    ReplacedPortableQuitNeeded,
    OpenedBrowser,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Returns `true` when Velvet is running inside an MSIX / Microsoft Store package.
pub fn is_packaged_msix() -> bool {
    let mut len = 0u32;
    let err = unsafe { GetCurrentPackageFullName(&mut len, PWSTR::null()) };
    err != APPMODEL_ERROR_NO_PACKAGE && (err.0 == 0 || err == ERROR_INSUFFICIENT_BUFFER)
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Parses `"v0.1.2"` or `"0.1.2"` (ignoring `-prerelease` if non-empty prerelease should not win).
fn parse_semver_triplet(raw: &str) -> Option<((u32, u32, u32), bool)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let (core, has_pre) = match trimmed.split_once('-') {
        Some((c, p)) => (c, !p.trim().is_empty()),
        None => (trimmed, false),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some(((major, minor, patch), has_pre))
}

pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let Some((cur, _)) = parse_semver_triplet(current) else {
        return false;
    };
    let Some((cand, cand_pre)) = parse_semver_triplet(candidate) else {
        return false;
    };
    if cand_pre {
        return false;
    }
    cand > cur
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn parse_https_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("https://")?;
    let (host_port, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()?),
        None => (host_port.to_string(), 443),
    };
    Some((host, port, path))
}

struct WinHttpHandle(*mut c_void);

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn winhttp_get(url: &str, accept_json: bool, mut sink: impl FnMut(&[u8]) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let (host, port, path) = parse_https_url(url).ok_or_else(|| anyhow!("Unsupported URL: {url}"))?;
    let host_w = to_wide_null(&host);
    let path_w = to_wide_null(&path);

    let session = WinHttpHandle(unsafe {
        WinHttpOpen(
            w!("Velvet/0.1"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(anyhow!("WinHttpOpen failed"));
    }

    let connect = WinHttpHandle(unsafe { WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0) });
    if connect.0.is_null() {
        return Err(anyhow!("WinHttpConnect failed"));
    }

    let request = WinHttpHandle(unsafe {
        WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(anyhow!("WinHttpOpenRequest failed"));
    }

    let headers_w = accept_json.then(|| to_wide_null("Accept: application/vnd.github+json\r\n"));
    let headers_slice = headers_w.as_deref().map(|h| &h[..h.len() - 1]);

    unsafe {
        WinHttpSendRequest(request.0, headers_slice, None, 0, 0, 0)
            .context("WinHttpSendRequest failed")?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .context("WinHttpReceiveResponse failed")?;

        let mut status_code = 0u32;
        let mut status_size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status_code as *mut u32 as *mut _),
            &mut status_size,
            std::ptr::null_mut(),
        )
        .context("WinHttpQueryHeaders failed")?;

        if !(200..300).contains(&status_code) {
            return Err(anyhow!("HTTP status {status_code} for {url}"));
        }

        let mut buf = [0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut read,
            )
            .context("WinHttpReadData failed")?;
            if read == 0 {
                break;
            }
            sink(&buf[..read as usize])?;
        }
    }

    Ok(())
}

pub fn parse_release_json(raw: &[u8], cur_ver: &str) -> Option<UpdateInfo> {
    let rel: GitHubRelease = serde_json::from_slice(raw).ok()?;
    if rel.draft || rel.prerelease || !is_newer_version(cur_ver, &rel.tag_name) {
        return None;
    }

    let clean_ver = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let installer = rel.assets.iter().find(|a| {
        let lower = a.name.to_ascii_lowercase();
        lower.ends_with(".exe") && lower.contains("setup")
    });
    let portable = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("velvet.exe"));

    Some(UpdateInfo {
        version: clean_ver,
        html_url: rel.html_url,
        installer_url: installer.map(|a| a.browser_download_url.clone()),
        installer_name: installer.map(|a| a.name.clone()),
        portable_url: portable.map(|a| a.browser_download_url.clone()),
    })
}

/// Checks GitHub's latest release. Returns `Ok(Some(info))` if a newer version exists.
pub fn check_for_update() -> anyhow::Result<Option<UpdateInfo>> {
    if is_packaged_msix() {
        return Ok(None);
    }
    let mut body = Vec::with_capacity(8192);
    winhttp_get(GITHUB_LATEST_RELEASE_URL, true, |chunk| {
        if body.len() + chunk.len() > 512 * 1024 {
            return Err(anyhow!("Release metadata exceeded 512 KiB"));
        }
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(parse_release_json(&body, current_version()))
}

fn is_installed_via_setup(exe_path: &Path) -> bool {
    let Some(dir) = exe_path.parent() else {
        return false;
    };
    if dir.join("unins000.exe").exists() {
        return true;
    }
    dir.to_string_lossy()
        .to_ascii_lowercase()
        .contains(r"\appdata\local\programs\velvet")
}

fn download_pe_binary(url: &str, dest: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(dest)?;
    let mut header = [0u8; 2];
    let mut total = 0usize;
    winhttp_get(url, false, |chunk| {
        if total < 2 {
            let take = (2 - total).min(chunk.len());
            header[total..total + take].copy_from_slice(&chunk[..take]);
        }
        total += chunk.len();
        file.write_all(chunk)?;
        Ok(())
    })?;
    file.flush()?;
    if total < 256 * 1024 || &header != b"MZ" {
        let _ = fs::remove_file(dest);
        return Err(anyhow!("Downloaded asset is not a valid Windows executable"));
    }
    Ok(())
}

/// Downloads and applies the given update (silent Inno Setup upgrade, portable in-place swap,
/// or browser fallback if downloading fails).
pub fn apply_update(info: &UpdateInfo) -> anyhow::Result<UpdateApplyResult> {
    if is_packaged_msix() {
        return Err(anyhow!("Updates are managed by Microsoft Store"));
    }
    let current_exe = std::env::current_exe()?;

    if is_installed_via_setup(&current_exe) {
        if let Some(ref url) = info.installer_url {
            let name = info
                .installer_name
                .as_deref()
                .unwrap_or("Velvet-Setup.exe");
            let temp_dir = std::env::temp_dir().join("velvet-update");
            let setup_path = temp_dir.join(name);
            download_pe_binary(url, &setup_path)?;

            // Run silent Inno Setup then relaunch the updated velvet.exe cleanly.
            let script = format!(
                "\"{}\" /SILENT /NORESTART && start \"\" \"{}\"",
                setup_path.display(),
                current_exe.display()
            );
            Command::new("cmd.exe")
                .args(["/C", &script])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .context("Failed to launch Velvet installer")?;
            return Ok(UpdateApplyResult::LaunchedInstallerQuitNeeded);
        }
    }

    if let Some(ref url) = info.portable_url {
        if let Some(dir) = current_exe.parent() {
            let new_path = dir.join("velvet.exe.new");
            let old_path = dir.join("velvet.exe.old");
            download_pe_binary(url, &new_path)?;
            let _ = fs::remove_file(&old_path);
            fs::rename(&current_exe, &old_path)
                .context("Failed to move running velvet.exe aside")?;
            if let Err(err) = fs::rename(&new_path, &current_exe) {
                let _ = fs::rename(&old_path, &current_exe);
                return Err(err.into());
            }
            Command::new(&current_exe).spawn()?;
            return Ok(UpdateApplyResult::ReplacedPortableQuitNeeded);
        }
    }

    crate::indexer::open_file_in_shell(Path::new(&info.html_url));
    Ok(UpdateApplyResult::OpenedBrowser)
}

/// Removes any leftover `velvet.exe.old` left behind by a prior portable self-update.
pub fn cleanup_old_binary() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Some(dir) = exe.parent() {
        let old = dir.join("velvet.exe.old");
        if old.exists() {
            let _ = fs::remove_file(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_tags_properly() {
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.9.9", "v1.0.0"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));
        assert!(!is_newer_version("0.2.0", "v0.1.9"));
        assert!(!is_newer_version("0.1.0", "v0.2.0-beta.1"));
    }

    #[test]
    fn parses_github_release_payload_and_selects_assets() {
        let json = br#"{
          "tag_name": "v0.2.0",
          "html_url": "https://github.com/Starfalleso/Velvet/releases/tag/v0.2.0",
          "draft": false,
          "prerelease": false,
          "assets": [
            { "name": "Velvet-Setup-0.2.0.exe", "browser_download_url": "https://example.com/setup.exe" },
            { "name": "velvet.exe", "browser_download_url": "https://example.com/velvet.exe" }
          ]
        }"#;
        let info = parse_release_json(json, "0.1.0").expect("should detect newer release");
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.installer_url.as_deref(), Some("https://example.com/setup.exe"));
        assert_eq!(info.portable_url.as_deref(), Some("https://example.com/velvet.exe"));
        assert!(parse_release_json(json, "0.2.0").is_none());
    }

    #[test]
    fn msix_guard_reports_false_for_unpacked_binary() {
        assert!(!is_packaged_msix());
    }
}
