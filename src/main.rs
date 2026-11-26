#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod config;
mod icons;
mod indexer;
mod ipc;
mod math;
mod search;
mod taskbar;
mod themes;
mod ui;
mod usage;
mod win_hooks;
mod win_window;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    px, size, AppContext, Application, Bounds, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};

use crate::commands::AppCommand;
use crate::config::Config;
use crate::ui::LauncherView;

#[derive(Debug, Default)]
struct CliArgs {
    toggle: bool,
    show: bool,
    hide: bool,
    show_taskbar: bool,
    hide_taskbar: bool,
    toggle_taskbar: bool,
    reload: bool,
    quit: bool,
    help: bool,
    config_path: Option<PathBuf>,
    theme_slug: Option<String>,
}

fn parse_args() -> CliArgs {
    let mut args = CliArgs::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--toggle" | "-t" => args.toggle = true,
            "--show" => args.show = true,
            "--hide" => args.hide = true,
            "--show-taskbar" => args.show_taskbar = true,
            "--hide-taskbar" => args.hide_taskbar = true,
            "--toggle-taskbar" => args.toggle_taskbar = true,
            "--reload" => args.reload = true,
            "--quit" => args.quit = true,
            "--help" | "-h" => args.help = true,
            "--config" | "-c" => {
                if let Some(p) = iter.next() {
                    args.config_path = Some(PathBuf::from(p));
                }
            }
            "--theme" => {
                if let Some(t) = iter.next() {
                    args.theme_slug = Some(t);
                }
            }
            _ => {}
        }
    }
    args
}

fn print_help() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    println!(
        "velvet - Fast Windows 11 App Launcher\n\n\
         USAGE:\n\
           velvet.exe [OPTIONS]\n\n\
         OPTIONS:\n\
           --toggle, -t       Toggle launcher window on the running instance\n\
           --show             Show launcher window\n\
           --hide             Hide launcher window\n\
           --hide-taskbar     Hide the Windows 11 taskbar and reclaim work area\n\
           --show-taskbar     Restore the Windows 11 taskbar and work area\n\
           --toggle-taskbar   Toggle Windows 11 taskbar visibility\n\
           --reload           Re-scan installed applications\n\
           --quit             Exit the running instance cleanly (restoring taskbar)\n\
           --theme NAME       Switch theme live (carbon-dark, tokyo-night, catppuccin-mocha)\n\
           --config, -c PATH  Use a custom config.toml path\n\
           --help, -h         Print this help message"
    );
}

fn spawn_indexer_thread(search_cfg: config::SearchConfig, tx: async_channel::Sender<AppCommand>) {
    std::thread::Builder::new()
        .name("pill-app-indexer".to_string())
        .spawn(move || {
            indexer::index_all_apps_streaming(&search_cfg, |apps| {
                let _ = tx.send_blocking(AppCommand::AppsIndexed(Arc::new(apps)));
            });
        })
        .ok();
}

fn main() {
    let args = parse_args();
    if args.help {
        print_help();
        return;
    }

    // Handle single-instance IPC forwarding first.
    if args.show_taskbar {
        if ipc::try_send_command("SHOW_TASKBAR") {
            return;
        }
        taskbar::restore_taskbar();
        return;
    }
    if args.quit {
        let _ = ipc::try_send_command("QUIT");
        return;
    }
    if args.hide {
        let _ = ipc::try_send_command("HIDE");
        return;
    }
    if let Some(ref slug) = args.theme_slug {
        if ipc::try_send_command(&format!("THEME:{slug}")) {
            return;
        }
    }
    if args.reload {
        let _ = ipc::try_send_command("RELOAD");
        return;
    }
    if args.toggle_taskbar {
        if ipc::try_send_command("TOGGLE_TASKBAR") {
            return;
        }
    }
    if args.hide_taskbar {
        if ipc::try_send_command("HIDE_TASKBAR") {
            return;
        }
    }
    if args.toggle {
        if ipc::try_send_command("TOGGLE") {
            return;
        }
    }
    if args.show {
        if ipc::try_send_command("SHOW") {
            return;
        }
    }
    // Plain launch with no action flags: if already running, surface the window & exit.
    if !args.toggle
        && !args.show
        && !args.hide_taskbar
        && !args.toggle_taskbar
        && ipc::try_send_command("SHOW")
    {
        return;
    }

    // Install panic hook & RAII guard so the Windows taskbar can never remain stuck hidden.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        taskbar::restore_if_hidden();
        if let Some(dir) = dirs::config_dir() {
            let log_dir = dir.join("velvet");
            let _ = std::fs::create_dir_all(&log_dir);
            let _ = std::fs::write(log_dir.join("last-panic.log"), format!("{info}\n"));
        }
        prev_hook(info);
    }));
    let _taskbar_guard = taskbar::TaskbarGuard::new();

    let (config, config_path): (Config, PathBuf) =
        config::load_or_create(args.config_path.as_deref());

    if config.taskbar.hide_on_startup || args.hide_taskbar {
        taskbar::hide_taskbar();
    } else if args.toggle_taskbar {
        taskbar::toggle_taskbar();
    } else if taskbar::has_stale_hidden_flag() {
        taskbar::restore_taskbar();
    }

    let (cmd_tx, cmd_rx) = async_channel::unbounded::<AppCommand>();
    ipc::spawn_ipc_server(cmd_tx.clone());
    config::spawn_config_watcher(config_path.clone(), cmd_tx.clone());
    let hooks_handle = win_hooks::spawn(&config, cmd_tx.clone());
    spawn_indexer_thread(config.search.clone(), cmd_tx.clone());
    indexer::spawn_start_menu_watcher(cmd_tx.clone(), config.search.clone());

    let show_on_boot = args.toggle || args.show;
    let initial_width = config.window.width + 2.0 * win_window::WINDOW_PAD_X;
    let initial_height =
        config.window.max_height + win_window::WINDOW_PAD_TOP + win_window::WINDOW_PAD_BOTTOM;
    let search_cfg_for_reload = config.search.clone();
    let config_path_for_open = config_path.clone();

    Application::new().run(move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(initial_width), px(initial_height)),
                cx,
            ))),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };

        let Ok(window_handle) = cx.open_window(options, |window, cx| {
            let hwnd = window
                .window_handle()
                .ok()
                .and_then(|wh| match wh.as_raw() {
                    RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
                    _ => None,
                });

            if let Some(h) = hwnd {
                win_window::configure_launcher_hwnd(h);
                win_hooks::set_launcher_hwnd(h);
            }

            let tx_clone = cmd_tx.clone();
            cx.new(|cx| {
                LauncherView::new(
                    config.clone(),
                    config_path.clone(),
                    hwnd,
                    tx_clone,
                    window,
                    cx,
                )
            })
        }) else {
            taskbar::restore_if_hidden();
            hooks_handle.shutdown();
            cx.quit();
            return;
        };

        if show_on_boot {
            let _ = window_handle.update(cx, |view, window, cx| {
                view.show(window, cx);
            });
        }

        let cmd_tx_reload = cmd_tx.clone();
        cx.spawn(async move |cx| {
            while let Ok(cmd) = cmd_rx.recv().await {
                match cmd {
                    AppCommand::Toggle => {
                        let _ = window_handle.update(cx, |view, window, cx| {
                            view.toggle(window, cx);
                        });
                    }
                    AppCommand::Show => {
                        let _ = window_handle.update(cx, |view, window, cx| {
                            view.show(window, cx);
                        });
                    }
                    AppCommand::Hide => {
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.hide(cx);
                        });
                    }
                    AppCommand::ReloadApps => {
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.set_indexing(true, cx);
                        });
                        spawn_indexer_thread(
                            search_cfg_for_reload.clone(),
                            cmd_tx_reload.clone(),
                        );
                    }
                    AppCommand::AppsIndexed(apps_arc) => {
                        let apps_vec = (*apps_arc).clone();
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.set_apps(apps_vec, cx);
                        });
                    }
                    AppCommand::ReloadConfig(cfg) => {
                        hooks_handle.set_intercept_win_key(cfg.hotkeys.intercept_win_key);
                        win_hooks::set_max_tap_ms(cfg.hotkeys.win_key_max_tap_ms);
                        win_hooks::set_active_theme_from_config(&cfg.theme);
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.apply_config(*cfg, cx);
                        });
                    }
                    AppCommand::ApplyTheme(preset) => {
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.apply_theme_preset(preset, cx);
                        });
                    }
                    AppCommand::ToggleWinKeyHook => {
                        let mut enabled_res = None;
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            enabled_res = Some(view.toggle_win_key_hook(cx));
                        });
                        if let Some(enabled) = enabled_res {
                            hooks_handle.set_intercept_win_key(enabled);
                        }
                    }
                    AppCommand::ToggleAutostart => {
                        let enabled = autostart::toggle();
                        let msg = if enabled {
                            "Autostart: On".to_string()
                        } else {
                            "Autostart: Off".to_string()
                        };
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.set_status_hint(msg, cx);
                        });
                    }
                    AppCommand::ShowTaskbar => {
                        let tx_done = cmd_tx_reload.clone();
                        let hooks = hooks_handle;
                        std::thread::spawn(move || {
                            taskbar::restore_taskbar();
                            hooks.sync_taskbar_watch(taskbar::is_hidden());
                            let _ = tx_done.send_blocking(AppCommand::TaskbarStateChanged);
                        });
                    }
                    AppCommand::HideTaskbar => {
                        let tx_done = cmd_tx_reload.clone();
                        let hooks = hooks_handle;
                        std::thread::spawn(move || {
                            taskbar::hide_taskbar();
                            hooks.sync_taskbar_watch(taskbar::is_hidden());
                            let _ = tx_done.send_blocking(AppCommand::TaskbarStateChanged);
                        });
                    }
                    AppCommand::ToggleTaskbar => {
                        let tx_done = cmd_tx_reload.clone();
                        let hooks = hooks_handle;
                        std::thread::spawn(move || {
                            taskbar::toggle_taskbar();
                            hooks.sync_taskbar_watch(taskbar::is_hidden());
                            let _ = tx_done.send_blocking(AppCommand::TaskbarStateChanged);
                        });
                    }
                    AppCommand::TaskbarStateChanged => {
                        let _ = window_handle.update(cx, |view, _window, cx| {
                            view.refresh_taskbar_state(cx);
                        });
                    }
                    AppCommand::OpenConfig => {
                        let path = config_path_for_open.clone();
                        std::thread::spawn(move || {
                            indexer::open_file_in_shell(&path);
                        });
                    }
                    AppCommand::Quit => {
                        taskbar::restore_if_hidden();
                        hooks_handle.shutdown();
                        let _ = cx.update(|cx| cx.quit());
                        break;
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });

    taskbar::restore_if_hidden();
}
