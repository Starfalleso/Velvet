use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::thread;

use async_channel::Sender;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::StationsAndDesktops::{
    GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

use crate::commands::AppCommand;
use crate::themes;

fn sanitize_tag(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Name of the window station desktop the calling thread is attached to
/// (`Default` for a normal interactive session). Empty if it cannot be queried.
fn current_desktop_tag() -> String {
    unsafe {
        let Ok(desktop) = GetThreadDesktop(GetCurrentThreadId()) else {
            return String::new();
        };
        let mut buf = [0u16; 256];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(buf.as_mut_ptr() as *mut _),
            (buf.len() * std::mem::size_of::<u16>()) as u32,
            Some(&mut needed),
        );
        if ok.is_err() {
            return String::new();
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }
}

/// Pipe name scoped to the user *and* the current desktop. Without the desktop
/// component, an instance stranded on another (e.g. sandboxed, invisible)
/// desktop would swallow `--toggle`/plain launches on the real desktop and
/// Velvet would appear to never open.
fn pipe_name_wide() -> Vec<u16> {
    let user = sanitize_tag(&std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string()));
    let desktop = sanitize_tag(&current_desktop_tag());
    let name = if desktop.is_empty() {
        format!(r"\\.\pipe\velvet-v1-{user}")
    } else {
        format!(r"\\.\pipe\velvet-v1-{user}-{desktop}")
    };
    OsStr::new(&name).encode_wide().chain(Some(0)).collect()
}

/// Attempts to send a single-line command to an already running instance.
/// Returns `true` if a running instance accepted the command.
pub fn try_send_command(cmd: &str) -> bool {
    let pipe_wide = pipe_name_wide();
    let pcw = PCWSTR(pipe_wide.as_ptr());

    unsafe {
        let mut handle = CreateFileW(
            pcw,
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            Default::default(),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        );

        if handle.is_err() {
            if GetLastError() == ERROR_PIPE_BUSY {
                let _ = WaitNamedPipeW(pcw, 200);
                handle = CreateFileW(
                    pcw,
                    FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                    Default::default(),
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                );
            }
        }

        let Ok(handle) = handle else {
            return false;
        };
        if handle == INVALID_HANDLE_VALUE {
            return false;
        }

        let _ = AllowSetForegroundWindow(ASFW_ANY);

        let payload = format!("{cmd}\n");
        let mut written = 0u32;
        let ok = WriteFile(handle, Some(payload.as_bytes()), Some(&mut written), None).is_ok();
        let _ = FlushFileBuffers(handle);
        let _ = CloseHandle(handle);
        ok && written > 0
    }
}

/// Spawns a background thread listening on the named pipe for single-instance commands.
pub fn spawn_ipc_server(tx: Sender<AppCommand>) {
    thread::Builder::new()
        .name("pill-ipc-server".to_string())
        .spawn(move || {
            let pipe_wide = pipe_name_wide();
            let pcw = PCWSTR(pipe_wide.as_ptr());

            loop {
                let handle: HANDLE = unsafe {
                    CreateNamedPipeW(
                        pcw,
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                        16,
                        512,
                        512,
                        0,
                        None,
                    )
                };

                if handle == INVALID_HANDLE_VALUE {
                    thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }

                let connected = unsafe {
                    match ConnectNamedPipe(handle, None) {
                        Ok(()) => true,
                        Err(_) => GetLastError() == ERROR_PIPE_CONNECTED,
                    }
                };

                if connected {
                    let mut buf = [0u8; 256];
                    let mut read_bytes = 0u32;
                    let ok = unsafe {
                        ReadFile(handle, Some(&mut buf), Some(&mut read_bytes), None).is_ok()
                    };
                    if ok && read_bytes > 0 {
                        if let Ok(msg) = std::str::from_utf8(&buf[..read_bytes as usize]) {
                            for line in msg.lines() {
                                if let Some(cmd) = parse_ipc_command(line.trim()) {
                                    let _ = tx.send_blocking(cmd);
                                }
                            }
                        }
                    }
                    unsafe {
                        let _ = DisconnectNamedPipe(handle);
                    }
                }

                unsafe {
                    let _ = CloseHandle(handle);
                }
            }
        })
        .ok();
}

fn parse_ipc_command(s: &str) -> Option<AppCommand> {
    if let Some(slug) = s.strip_prefix("THEME:").or_else(|| s.strip_prefix("theme:")) {
        return themes::find_theme(slug).map(AppCommand::ApplyTheme);
    }
    match s.to_ascii_uppercase().as_str() {
        "TOGGLE" => Some(AppCommand::Toggle),
        "SHOW" => Some(AppCommand::Show),
        "HIDE" => Some(AppCommand::Hide),
        "RELOAD" => Some(AppCommand::ReloadApps),
        "SHOW_TASKBAR" => Some(AppCommand::ShowTaskbar),
        "HIDE_TASKBAR" => Some(AppCommand::HideTaskbar),
        "TOGGLE_TASKBAR" => Some(AppCommand::ToggleTaskbar),
        "OPEN_CONFIG" => Some(AppCommand::OpenConfig),
        "QUIT" => Some(AppCommand::Quit),
        _ => None,
    }
}
