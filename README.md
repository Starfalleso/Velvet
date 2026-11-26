# Velvet

A fast, lightweight Windows 11 app launcher built in Rust with **GPUI**.

## Features
- **Instant Toggle & Single-Instance IPC:** Runs quietly in the system tray so toggling via hotkey or `--toggle` opens immediately with no cold-start flash.
- **Custom Icon & EXE Resource:** Uses [assets/velvet.svg](assets/velvet.svg) supersampled into the 64×64 tray icon and embedded multi-resolution [assets/velvet.ico](assets/velvet.ico) via [build.rs](build.rs).
- **App Indexing, Pinned Favorites & Fuzzy Search:** Two-phase streaming indexer for Start Menu shortcuts (`.lnk`/`.exe`) and Microsoft Store / UWP apps (`shell:AppsFolder`), OS-event Start Menu auto-rescan (`FindFirstChangeNotificationW`), fuzzy match character highlighting, launch frequency ranking, and **pinned favorite apps** (row pin icon or `Ctrl+P`).
- **Command Runner & Admin Launch:** Type `> <command>` to execute commands or open paths/URLs directly, and press **`Shift+Enter`** on desktop apps or commands to run elevated as Administrator.
- **Full Search Bar Selection:** Supports click-and-drag highlighting, horizontal scroll tracking on long queries, double-click word select, triple-click / `Ctrl+A` select all, `Shift+Arrows`, `Ctrl+C` / `Ctrl+X` / `Ctrl+V`, and replacing/deleting selected text directly.
- **Quick Math:** Type expressions like `24*12`, `= 2+2`, `sqrt(144)`, or `0xff + 1` and press `Enter` to copy the result to your clipboard.
- **System & Power Actions:** Lock Screen, Sleep, two-step confirmed Restart / Shut Down, Reload App Index, Open Launcher Settings, Toggle Windows Key Hook, and Hide/Show Windows Taskbar.
- **System Tray Menu:** Right-click the Velvet tray icon to switch presets live (`Theme ▸`), toggle **Intercept Windows Key**, toggle **Hide Windows Taskbar**, toggle **Start with Windows**, reload apps, open `config.toml`, or quit cleanly.
- **Global Hotkey + Optional Lone `Win` Key Hook:** Defaults to `Alt+Space`; optionally intercepts a clean lone `Win` key tap while letting all `Win+*` shortcuts pass through (toggleable live from the tray menu or launcher search without restarting).
- **Live Theme Switcher, Custom Themes & YASB Sync:** Switch between **Carbon Dark** (default), **Tokyo Night**, and **Catppuccin Mocha**—plus any drop-in `.toml` files in `%APPDATA%\velvet\themes\`—live inside Velvet, from the tray menu, or via `--theme <name>` (with optional live YASB bar syncing via `sync_yasb = true`).
- **`config.toml` Hot-Reload:** Edits to `config.toml` (including `intercept_win_key` and theme colors) take effect automatically within ~750 ms.
- **Click-Outside Dismissal:** Global `WH_MOUSE_LL` hook + window blur handling closes Velvet smoothly when clicking anywhere outside.
- **Taskbar Hide Mode:** Reliably hides Explorer's `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` (filtering by `explorer.exe` so third-party dummy `Shell_TrayWnd` hosts like YASB's systray listener are never touched) whether Windows auto-hide is ON or OFF—combining `ABM_SETSTATE`, `EnableWindow(false)`, `SW_HIDE` + `SWP_HIDEWINDOW`, an event-driven `SetWinEventHook` + `250 ms` re-hide watchdog (without triggering `SPI_SETWORKAREA` loops), and initial edge-aware `SPI_SETWORKAREA` expansion that preserves top AppBars (like YASB), with guaranteed restoration on exit, panic, or next startup via `taskbar-hidden.flag`.

## Build
```powershell
cargo build --release
```
The release binary is output at `target\release\velvet.exe`.

- Architecture details & diagrams: [ARCHITECTURE.md](ARCHITECTURE.md)
- Saved theme presets (Carbon Dark, Tokyo Night, Catppuccin Mocha): [THEMES.md](THEMES.md)

---

## Shortcuts & Controls

| Action | Shortcut / Interaction |
| :--- | :--- |
| Open / Close Velvet | `Alt+Space` (configurable) or lone `Win` key tap (when enabled) |
| Move selection | `↑` / `↓`, `Tab` / `Shift+Tab`, `PageUp` / `PageDown` |
| Launch app / copy math / run action | `Enter` or left-click row (`Shift+Enter` runs as Administrator) |
| Run shell command or open URL/path | Prefix query with `>` (e.g. `> wt`, `> ipconfig`, `> https://...`) |
| Pin / Unpin favorite app | `Ctrl+P` or click the pin icon on a row |
| Select text in search bar | Click & drag, double-click (word), triple-click / `Ctrl+A` (all), `Shift+Arrows` |
| Copy / Cut / Paste / Clear query | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` / `Ctrl+U` or `Ctrl+L` |
| Dismiss | `Esc` or click anywhere outside |

---

## 1. Run on Windows Startup
- **One-click (Recommended):** Right-click the **Velvet** icon in your system tray and toggle **Start with Windows** (automatically creates/removes `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Velvet.lnk`).
- **Manual shortcut:** Press `Win+R`, type `shell:startup`, press `Enter`, and create a shortcut pointing to:
  ```text
  C:\path\to\velvet.exe
  ```
  *(Or pass `--hide-taskbar` at the end if you only want taskbar hiding via shortcut flags instead of `config.toml`.)*

---

## 2. Hook `velvet.exe --toggle` into YASB
In `%USERPROFILE%\.config\yasb\config.yaml`, configure your `launchpad` widget as a `yasb.custom.CustomWidget`:

```yaml
  launchpad:
    type: "yasb.custom.CustomWidget"
    options:
      label: "<span>\udb80\udd3b</span>"
      label_alt: "<span>\udb80\udd3b</span>"
      class_name: "launchpad-widget"
      callbacks:
        on_left: "exec C:\\path\\to\\velvet.exe --toggle"
        on_middle: "do_nothing"
        on_right: "exec C:\\path\\to\\velvet.exe --toggle-taskbar"
```

Because `velvet.exe --toggle` communicates over a Win32 Named Pipe scoped to your user and Windows desktop (`\\.\pipe\velvet-v1-<user>-<desktop>`), clicking the YASB bar button toggles the running instance instantaneously without spawning a second window.

---

## 3. Configure `config.toml`
On first launch, `velvet.exe` creates:
`%APPDATA%\velvet\config.toml`
*(Or place a `config.toml` next to `velvet.exe` for portable mode; see [config.example.toml](config.example.toml) and [THEMES.md](THEMES.md).)*

Key options:
- **Hotkeys & `Win`-key hook (`[hotkeys]`):**
  - `toggle = "Alt+Space"` (supports `Alt`, `Ctrl`, `Shift`, `Win` + key)
  - `intercept_win_key = true` to toggle on a clean lone `Win` press-and-release (`win_key_max_tap_ms = 350`)
- **Taskbar hiding (`[taskbar]`):**
  - `hide_on_startup = true` to hide the Windows 11 taskbar automatically when Velvet starts
  - Emergency CLI restore anytime: `velvet.exe --show-taskbar`
- **Theme (`[theme]`):**
  - `sync_yasb = true` (off by default) syncs YASB (`~/.config/yasb/styles.css` and `config.yaml`) live whenever you switch presets inside Velvet (`Theme: ...`) or via `velvet.exe --theme tokyo-night`.
  - `font_family`, `mono_font_family`, `font_size`, and `#RRGGBB` / `#RRGGBBAA` hex colors (hot-reloaded automatically when saved).
