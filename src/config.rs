use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use async_channel::Sender;
use gpui::{Hsla, Rgba};
use serde::{Deserialize, Serialize};

use crate::commands::AppCommand;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
    VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub window: WindowConfig,
    pub hotkeys: HotkeysConfig,
    pub taskbar: TaskbarConfig,
    pub search: SearchConfig,
    pub update: UpdateConfig,
    pub theme: ThemeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            hotkeys: HotkeysConfig::default(),
            taskbar: TaskbarConfig::default(),
            search: SearchConfig::default(),
            update: UpdateConfig::default(),
            theme: ThemeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    pub check_updates: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_updates: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: f32,
    pub max_height: f32,
    pub top_offset_ratio: f32,
    pub border_radius: f32,
    pub hide_on_blur: bool,
    pub row_height: f32,
    pub animation_ms: u64,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 580.0,
            max_height: 412.0,
            top_offset_ratio: 0.22,
            border_radius: 12.0,
            hide_on_blur: true,
            row_height: 40.0,
            animation_ms: 130,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysConfig {
    pub toggle: String,
    pub intercept_win_key: bool,
    pub win_key_max_tap_ms: u64,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            toggle: "Alt+Space".to_string(),
            intercept_win_key: false,
            win_key_max_tap_ms: 350,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskbarConfig {
    pub hide_on_startup: bool,
    pub watchdog_interval_ms: u32,
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            hide_on_startup: false,
            watchdog_interval_ms: 1500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub max_results: usize,
    pub show_math: bool,
    pub show_system_actions: bool,
    pub index_uwp: bool,
    pub index_start_menu: bool,
    pub placeholder: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 40,
            show_math: true,
            show_system_actions: true,
            index_uwp: true,
            index_start_menu: true,
            placeholder: "Search apps, calculate, or run actions...".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub sync_yasb: bool,
    pub font_family: String,
    pub mono_font_family: String,
    pub font_size: f32,
    pub background: String,
    pub surface: String,
    pub surface_focused: String,
    pub row_hover: String,
    pub row_selected: String,
    pub border: String,
    pub border_subtle: String,
    pub accent: String,
    pub accent_secondary: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub badge_app: String,
    pub badge_app_text: String,
    pub badge_store: String,
    pub badge_store_text: String,
    pub badge_math: String,
    pub badge_math_text: String,
    pub badge_system: String,
    pub badge_system_text: String,
}

impl Default for ThemeConfig {
    /// Defaults to the built-in "Carbon Dark" theme.
    fn default() -> Self {
        Self {
            sync_yasb: false,
            font_family: "Poppins".to_string(),
            mono_font_family: "JetBrainsMono NFP".to_string(),
            font_size: 13.5,
            background: "#0b0b0ef6".to_string(),
            surface: "#08080af4".to_string(),
            surface_focused: "#0b0b0ef6".to_string(),
            row_hover: "#1a1a20cc".to_string(),
            row_selected: "#24242be8".to_string(),
            border: "#ffffff14".to_string(),
            border_subtle: "#ffffff0c".to_string(),
            accent: "#e4e4e7ff".to_string(),
            accent_secondary: "#a1a1aaff".to_string(),
            text_primary: "#f4f4f5ff".to_string(),
            text_secondary: "#a1a1aaff".to_string(),
            text_muted: "#71717aff".to_string(),
            badge_app: "#e4e4e722".to_string(),
            badge_app_text: "#e4e4e7ff".to_string(),
            badge_store: "#7dd3fc22".to_string(),
            badge_store_text: "#7dd3fcff".to_string(),
            badge_math: "#6ee7b722".to_string(),
            badge_math_text: "#6ee7b7ff".to_string(),
            badge_system: "#fca5a522".to_string(),
            badge_system_text: "#fca5a5ff".to_string(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    pub font_family: String,
    pub mono_font_family: String,
    pub font_size: f32,
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_focused: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    pub border: Hsla,
    pub border_subtle: Hsla,
    pub accent: Hsla,
    pub accent_secondary: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub badge_app: Hsla,
    pub badge_app_text: Hsla,
    pub badge_store: Hsla,
    pub badge_store_text: Hsla,
    pub badge_math: Hsla,
    pub badge_math_text: Hsla,
    pub badge_system: Hsla,
    pub badge_system_text: Hsla,
}

impl ThemeConfig {
    pub fn resolve(&self) -> ResolvedTheme {
        let d = ThemeConfig::default();
        ResolvedTheme {
            font_family: self.font_family.clone(),
            mono_font_family: self.mono_font_family.clone(),
            font_size: self.font_size.clamp(10.0, 24.0),
            background: parse_hex_color(&self.background, &d.background),
            surface: parse_hex_color(&self.surface, &d.surface),
            surface_focused: parse_hex_color(&self.surface_focused, &d.surface_focused),
            row_hover: parse_hex_color(&self.row_hover, &d.row_hover),
            row_selected: parse_hex_color(&self.row_selected, &d.row_selected),
            border: parse_hex_color(&self.border, &d.border),
            border_subtle: parse_hex_color(&self.border_subtle, &d.border_subtle),
            accent: parse_hex_color(&self.accent, &d.accent),
            accent_secondary: parse_hex_color(&self.accent_secondary, &d.accent_secondary),
            text_primary: parse_hex_color(&self.text_primary, &d.text_primary),
            text_secondary: parse_hex_color(&self.text_secondary, &d.text_secondary),
            text_muted: parse_hex_color(&self.text_muted, &d.text_muted),
            badge_app: parse_hex_color(&self.badge_app, &d.badge_app),
            badge_app_text: parse_hex_color(&self.badge_app_text, &d.badge_app_text),
            badge_store: parse_hex_color(&self.badge_store, &d.badge_store),
            badge_store_text: parse_hex_color(&self.badge_store_text, &d.badge_store_text),
            badge_math: parse_hex_color(&self.badge_math, &d.badge_math),
            badge_math_text: parse_hex_color(&self.badge_math_text, &d.badge_math_text),
            badge_system: parse_hex_color(&self.badge_system, &d.badge_system),
            badge_system_text: parse_hex_color(&self.badge_system_text, &d.badge_system_text),
        }
    }
}

/// Parses `#RRGGBB` or `#RRGGBBAA` into `gpui::Hsla`, falling back to `fallback`.
pub fn parse_hex_color(raw: &str, fallback: &str) -> Hsla {
    parse_hex_rgba(raw)
        .or_else(|| parse_hex_rgba(fallback))
        .unwrap_or_else(|| Rgba { r: 0.043, g: 0.043, b: 0.055, a: 0.96 }.into())
}

pub fn parse_hex_rgba(raw: &str) -> Option<Hsla> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    let (r, g, b, a) = match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };

    Some(
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
        .into(),
    )
}

/// Parses a hotkey string such as `"Alt+Space"` or `"Ctrl+Shift+P"` into Win32 `(modifiers, vk)`.
pub fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut mods: u32 = 0;
    let mut vk: Option<u32> = None;

    for part in trimmed.split('+') {
        let token = part.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        match token.as_str() {
            "alt" => mods |= MOD_ALT.0,
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "super" | "meta" | "cmd" => mods |= MOD_WIN.0,
            key => {
                vk = parse_vk(key);
            }
        }
    }

    vk.map(|code| (mods, code))
}

fn parse_vk(key: &str) -> Option<u32> {
    match key {
        "space" => Some(VK_SPACE.0 as u32),
        "tab" => Some(VK_TAB.0 as u32),
        "enter" | "return" => Some(VK_RETURN.0 as u32),
        "esc" | "escape" => Some(VK_ESCAPE.0 as u32),
        "backspace" => Some(VK_BACK.0 as u32),
        "delete" | "del" => Some(VK_DELETE.0 as u32),
        "insert" | "ins" => Some(VK_INSERT.0 as u32),
        "home" => Some(VK_HOME.0 as u32),
        "end" => Some(VK_END.0 as u32),
        "pageup" | "pgup" => Some(VK_PRIOR.0 as u32),
        "pagedown" | "pgdn" => Some(VK_NEXT.0 as u32),
        "up" => Some(VK_UP.0 as u32),
        "down" => Some(VK_DOWN.0 as u32),
        "left" => Some(VK_LEFT.0 as u32),
        "right" => Some(VK_RIGHT.0 as u32),
        "f1" => Some(VK_F1.0 as u32),
        "f2" => Some(VK_F2.0 as u32),
        "f3" => Some(VK_F3.0 as u32),
        "f4" => Some(VK_F4.0 as u32),
        "f5" => Some(VK_F5.0 as u32),
        "f6" => Some(VK_F6.0 as u32),
        "f7" => Some(VK_F7.0 as u32),
        "f8" => Some(VK_F8.0 as u32),
        "f9" => Some(VK_F9.0 as u32),
        "f10" => Some(VK_F10.0 as u32),
        "f11" => Some(VK_F11.0 as u32),
        "f12" => Some(VK_F12.0 as u32),
        ";" | "semicolon" => Some(0xBA),
        "=" | "equal" => Some(0xBB),
        "," | "comma" => Some(0xBC),
        "-" | "minus" => Some(0xBD),
        "." | "period" => Some(0xBE),
        "/" | "slash" => Some(0xBF),
        "`" | "grave" | "tilde" => Some(0xC0),
        "[" | "lbracket" => Some(0xDB),
        "\\" | "backslash" => Some(0xDC),
        "]" | "rbracket" => Some(0xDD),
        "'" | "quote" => Some(0xDE),
        s if s.len() == 1 => {
            let b = s.as_bytes()[0];
            if b.is_ascii_lowercase() {
                Some(b.to_ascii_uppercase() as u32)
            } else if b.is_ascii_digit() {
                Some(b as u32)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn default_appdata_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("velvet")
        .join("config.toml")
}

/// Resolves config path in priority order:
/// 1. Explicit `--config <path>`
/// 2. `<exe_dir>/config.toml` (if it exists)
/// 3. `%APPDATA%\velvet\config.toml` (auto-created on first run if missing)
pub fn resolve_config_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let portable = dir.join("config.toml");
            if portable.exists() {
                return portable;
            }
        }
    }

    default_appdata_config_path()
}

pub fn load_or_create(explicit: Option<&Path>) -> (Config, PathBuf) {
    let path = resolve_config_path(explicit);
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(cfg) = toml::from_str::<Config>(&raw) {
                return (cfg, path);
            }
        }
    }

    let cfg = Config::default();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, render_default_toml(&cfg));
    }
    (cfg, path)
}

pub fn render_default_toml(cfg: &Config) -> String {
    let toml_body = toml::to_string_pretty(cfg).unwrap_or_default();
    format!(
        "# Velvet Configuration\n\
         # Default theme is Carbon Dark (see THEMES.md for presets).\n\
         # Colors accept #RRGGBB or #RRGGBBAA hex strings.\n\n\
         {toml_body}"
    )
}

pub fn save_config(path: &Path, cfg: &Config) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = render_default_toml(cfg);
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, &content)?;
    let _ = fs::rename(&tmp, path).or_else(|_| {
        fs::copy(&tmp, path)?;
        fs::remove_file(&tmp)
    });
    Ok(())
}

/// Polls `config.toml` modification time every 750ms and pushes `ReloadConfig` on valid edits.
pub fn spawn_config_watcher(path: PathBuf, tx: Sender<AppCommand>) {
    std::thread::Builder::new()
        .name("velvet-config-watcher".to_string())
        .spawn(move || {
            let mut last_mtime: Option<SystemTime> =
                fs::metadata(&path).and_then(|m| m.modified()).ok();
            loop {
                std::thread::sleep(Duration::from_millis(750));
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                let Ok(mtime) = meta.modified() else {
                    continue;
                };
                if last_mtime != Some(mtime) {
                    last_mtime = Some(mtime);
                    if let Ok(raw) = fs::read_to_string(&path) {
                        if let Ok(cfg) = toml::from_str::<Config>(&raw) {
                            let _ = tx.send_blocking(AppCommand::ReloadConfig(Box::new(cfg)));
                        }
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        let c6 = parse_hex_rgba("#cba6f7").unwrap();
        assert!((c6.a - 1.0).abs() < 0.01);
        let c8 = parse_hex_rgba("#110d1ef4").unwrap();
        assert!((c8.a - (0xf4 as f32 / 255.0)).abs() < 0.01);
        assert!(parse_hex_rgba("invalid").is_none());
    }

    #[test]
    fn parses_hotkeys() {
        let (mods, vk) = parse_hotkey("Alt+Space").unwrap();
        assert_eq!(mods, MOD_ALT.0);
        assert_eq!(vk, VK_SPACE.0 as u32);

        let (mods2, vk2) = parse_hotkey("Ctrl+Shift+K").unwrap();
        assert_eq!(mods2, MOD_CONTROL.0 | MOD_SHIFT.0);
        assert_eq!(vk2, b'K' as u32);

        assert!(parse_hotkey("").is_none());
    }

    #[test]
    fn round_trips_default_config() {
        let cfg = Config::default();
        let text = render_default_toml(&cfg);
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.hotkeys.toggle, "Alt+Space");
        assert_eq!(parsed.theme.font_family, "Poppins");
    }
}
