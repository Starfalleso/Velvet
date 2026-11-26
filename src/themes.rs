use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::ThemeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    CarbonDark,
    TokyoNight,
    CatppuccinMocha,
}

impl ThemePreset {
    pub fn all() -> &'static [ThemePreset] {
        &[
            ThemePreset::CarbonDark,
            ThemePreset::TokyoNight,
            ThemePreset::CatppuccinMocha,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ThemePreset::CarbonDark => "Carbon Dark",
            ThemePreset::TokyoNight => "Tokyo Night",
            ThemePreset::CatppuccinMocha => "Catppuccin Mocha",
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            ThemePreset::CarbonDark => "carbon-dark",
            ThemePreset::TokyoNight => "tokyo-night",
            ThemePreset::CatppuccinMocha => "catppuccin-mocha",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "carbon-dark" | "carbondark" | "carbon" | "dark" | "obsidian" => {
                Some(ThemePreset::CarbonDark)
            }
            "tokyo-night" | "tokyonight" | "tokyo" => Some(ThemePreset::TokyoNight),
            "catppuccin-mocha" | "catppuccin" | "mocha" => Some(ThemePreset::CatppuccinMocha),
            _ => None,
        }
    }

    pub fn keywords(&self) -> &'static str {
        match self {
            ThemePreset::CarbonDark => {
                "theme color palette preset carbon dark obsidian black neutral monochrome oled default"
            }
            ThemePreset::TokyoNight => "theme color palette preset tokyo night blue navy slate",
            ThemePreset::CatppuccinMocha => {
                "theme color palette preset catppuccin mocha mauve dark"
            }
        }
    }

    /// Returns `(background_hex, accent_hex)` for swatch previews.
    pub fn preview_colors(&self) -> (&'static str, &'static str) {
        match self {
            ThemePreset::CarbonDark => ("#0b0b0eff", "#e4e4e7ff"),
            ThemePreset::TokyoNight => ("#1a1b26ff", "#7aa2f7ff"),
            ThemePreset::CatppuccinMocha => ("#11111bff", "#cba6f7ff"),
        }
    }

    /// Builds a `ThemeConfig` for this preset while preserving font and `sync_yasb` settings.
    pub fn apply_to(&self, current: &ThemeConfig) -> ThemeConfig {
        let mut t = match self {
            ThemePreset::CarbonDark => ThemeConfig {
                font_family: current.font_family.clone(),
                mono_font_family: current.mono_font_family.clone(),
                font_size: current.font_size,
                sync_yasb: current.sync_yasb,
                ..ThemeConfig::default()
            },
            ThemePreset::TokyoNight => ThemeConfig {
                font_family: current.font_family.clone(),
                mono_font_family: current.mono_font_family.clone(),
                font_size: current.font_size,
                sync_yasb: current.sync_yasb,
                background: "#1a1b26f6".to_string(),
                surface: "#16161ef4".to_string(),
                surface_focused: "#1a1b26f6".to_string(),
                row_hover: "#24283bcc".to_string(),
                row_selected: "#292e42e8".to_string(),
                border: "#7aa2f71c".to_string(),
                border_subtle: "#41486838".to_string(),
                accent: "#7aa2f7ff".to_string(),
                accent_secondary: "#bb9af7ff".to_string(),
                text_primary: "#c0caf5ff".to_string(),
                text_secondary: "#a9b1d6ff".to_string(),
                text_muted: "#565f89ff".to_string(),
                badge_app: "#7aa2f722".to_string(),
                badge_app_text: "#7aa2f7ff".to_string(),
                badge_store: "#7dcfff22".to_string(),
                badge_store_text: "#7dcfffff".to_string(),
                badge_math: "#73daca22".to_string(),
                badge_math_text: "#73dacaff".to_string(),
                badge_system: "#f7768e22".to_string(),
                badge_system_text: "#f7768eff".to_string(),
            },
            ThemePreset::CatppuccinMocha => ThemeConfig {
                font_family: current.font_family.clone(),
                mono_font_family: current.mono_font_family.clone(),
                font_size: current.font_size,
                sync_yasb: current.sync_yasb,
                background: "#11111bf6".to_string(),
                surface: "#181825f2".to_string(),
                surface_focused: "#11111bf6".to_string(),
                row_hover: "#31324499".to_string(),
                row_selected: "#313244e6".to_string(),
                border: "#45475a38".to_string(),
                border_subtle: "#31324455".to_string(),
                accent: "#cba6f7ff".to_string(),
                accent_secondary: "#b4befeff".to_string(),
                text_primary: "#cdd6f4ff".to_string(),
                text_secondary: "#bac2deff".to_string(),
                text_muted: "#6c7086ff".to_string(),
                badge_app: "#cba6f722".to_string(),
                badge_app_text: "#cba6f7ff".to_string(),
                badge_store: "#89b4fa22".to_string(),
                badge_store_text: "#89b4faff".to_string(),
                badge_math: "#94e2d522".to_string(),
                badge_math_text: "#94e2d5ff".to_string(),
                badge_system: "#f38ba822".to_string(),
                badge_system_text: "#f38ba8ff".to_string(),
            },
        };
        t.sync_yasb = current.sync_yasb;
        t
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CustomYasbSync {
    pub restore_master_css: bool,
    pub circle_background_color: Option<String>,
    pub circle_work_progress_color: Option<String>,
    pub circle_break_progress_color: Option<String>,
    pub current_line_color: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomTheme {
    pub slug: String,
    pub name: String,
    pub keywords: String,
    pub config: ThemeConfig,
    pub yasb: Option<CustomYasbSync>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeChoice {
    Builtin(ThemePreset),
    Custom(CustomTheme),
}

impl ThemeChoice {
    pub fn name(&self) -> &str {
        match self {
            ThemeChoice::Builtin(p) => p.name(),
            ThemeChoice::Custom(c) => &c.name,
        }
    }

    pub fn slug(&self) -> &str {
        match self {
            ThemeChoice::Builtin(p) => p.slug(),
            ThemeChoice::Custom(c) => &c.slug,
        }
    }

    pub fn keywords(&self) -> String {
        match self {
            ThemeChoice::Builtin(p) => p.keywords().to_string(),
            ThemeChoice::Custom(c) => c.keywords.clone(),
        }
    }

    pub fn preview_colors(&self) -> (&str, &str) {
        match self {
            ThemeChoice::Builtin(p) => p.preview_colors(),
            ThemeChoice::Custom(c) => (&c.config.background, &c.config.accent),
        }
    }

    pub fn apply_to(&self, current: &ThemeConfig) -> ThemeConfig {
        match self {
            ThemeChoice::Builtin(p) => p.apply_to(current),
            ThemeChoice::Custom(c) => {
                let mut t = c.config.clone();
                t.font_family = current.font_family.clone();
                t.mono_font_family = current.mono_font_family.clone();
                t.font_size = current.font_size;
                t.sync_yasb = current.sync_yasb;
                t
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct CustomThemeFile {
    name: Option<String>,
    keywords: Option<String>,
    theme: Option<ThemeConfig>,
    yasb: Option<CustomYasbSync>,
}

fn title_case_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().to_string();
                    word.push_str(&chars.as_str().to_ascii_lowercase());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_custom_theme_str(slug: &str, raw: &str) -> Option<CustomTheme> {
    let clean_slug = slug.trim().to_ascii_lowercase();
    if clean_slug.is_empty() || ThemePreset::from_slug(&clean_slug).is_some() {
        return None;
    }

    let wrapper = toml::from_str::<CustomThemeFile>(raw).ok();
    let (mut name, extra_kw, theme_cfg, yasb) = match wrapper {
        Some(w) if w.theme.is_some() => (w.name, w.keywords, w.theme, w.yasb),
        Some(w) => {
            let has_color = raw.contains("background") || raw.contains("accent") || raw.contains("surface");
            let parsed_direct = has_color
                .then(|| toml::from_str::<ThemeConfig>(raw).ok())
                .flatten();
            (w.name, w.keywords, parsed_direct, w.yasb)
        }
        None => return None,
    };

    let cfg = theme_cfg?;
    let display_name = name
        .take()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| title_case_slug(&clean_slug));
    let mut kw = format!(
        "theme color palette preset custom {} {}",
        clean_slug,
        display_name.to_ascii_lowercase()
    );
    if let Some(extra) = extra_kw {
        let extra = extra.trim();
        if !extra.is_empty() {
            kw.push(' ');
            kw.push_str(extra);
        }
    }

    Some(CustomTheme {
        slug: clean_slug,
        name: display_name,
        keywords: kw,
        config: cfg,
        yasb,
    })
}

fn load_custom_themes_from_dir(dir: &Path, seen: &mut HashSet<String>, out: &mut Vec<CustomTheme>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("toml"))
            != Some(true)
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let slug = stem.trim().to_ascii_lowercase();
        if slug.is_empty() || seen.contains(&slug) {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Some(custom) = parse_custom_theme_str(&slug, &raw) {
                seen.insert(custom.slug.clone());
                out.push(custom);
            }
        }
    }
}

fn custom_theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("themes"));
        }
    }
    if let Some(cfg_dir) = dirs::config_dir() {
        dirs.push(cfg_dir.join("velvet").join("themes"));
    }
    dirs
}

/// Lists all built-in presets followed by any valid drop-in custom `.toml` themes.
pub fn list_themes() -> Vec<ThemeChoice> {
    let mut choices: Vec<ThemeChoice> = ThemePreset::all()
        .iter()
        .copied()
        .map(ThemeChoice::Builtin)
        .collect();

    let mut seen = HashSet::new();
    for preset in ThemePreset::all() {
        seen.insert(preset.slug().to_string());
    }
    let mut custom = Vec::new();
    for dir in custom_theme_dirs() {
        load_custom_themes_from_dir(&dir, &mut seen, &mut custom);
    }
    custom.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    choices.extend(custom.into_iter().map(ThemeChoice::Custom));
    choices
}

/// Resolves a built-in or custom theme by slug, alias, or display name.
pub fn find_theme(query: &str) -> Option<ThemeChoice> {
    if let Some(preset) = ThemePreset::from_slug(query) {
        return Some(ThemeChoice::Builtin(preset));
    }
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    list_themes()
        .into_iter()
        .find(|t| t.slug().eq_ignore_ascii_case(&q) || t.name().eq_ignore_ascii_case(&q))
}

/// Detects the active built-in or custom theme slug by matching `background` and `accent`.
pub fn detect_active_theme_slug(cfg: &ThemeConfig) -> Option<String> {
    let bg = cfg.background.trim();
    let accent = cfg.accent.trim();
    let base = ThemeConfig::default();
    list_themes().into_iter().find_map(|choice| {
        let applied = choice.apply_to(&base);
        (applied.background.trim().eq_ignore_ascii_case(bg)
            && applied.accent.trim().eq_ignore_ascii_case(accent))
        .then(|| choice.slug().to_string())
    })
}

/// Losslessly syncs YASB's `styles.css` (using a `styles.css.velvet.bak` master copy)
/// and inline widget colors in `config.yaml` if present under `%USERPROFILE%\.config\yasb`.
pub fn sync_yasb_theme(choice: &ThemeChoice) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let yasb_dir = home.join(".config").join("yasb");
    let css_path = yasb_dir.join("styles.css");
    let cfg_path = yasb_dir.join("config.yaml");

    if css_path.exists() {
        let master_path = yasb_dir.join("styles.css.velvet.bak");
        if !master_path.exists() {
            let _ = fs::copy(&css_path, &master_path);
        }
        match choice {
            ThemeChoice::Builtin(preset) => {
                if let Ok(master_css) = fs::read_to_string(&master_path) {
                    let rendered = match preset {
                        ThemePreset::CarbonDark => transform_css_carbon_dark(&master_css),
                        ThemePreset::TokyoNight => transform_css_tokyo_night(&master_css),
                        ThemePreset::CatppuccinMocha => transform_css_catppuccin_mocha(&master_css),
                    };
                    let _ = fs::write(&css_path, &rendered);
                }
            }
            ThemeChoice::Custom(custom) => {
                if custom
                    .yasb
                    .as_ref()
                    .map(|y| y.restore_master_css)
                    .unwrap_or(false)
                    && master_path.exists()
                {
                    let _ = fs::copy(&master_path, &css_path);
                }
            }
        }
    }

    if cfg_path.exists() {
        if let Ok(cfg_raw) = fs::read_to_string(&cfg_path) {
            let colors = match choice {
                ThemeChoice::Builtin(ThemePreset::CarbonDark) => Some(("#24242b", "#e4e4e7", "#6ee7b7", "#e4e4e7")),
                ThemeChoice::Builtin(ThemePreset::TokyoNight) => Some(("#292e42", "#7aa2f7", "#73daca", "#7aa2f7")),
                ThemeChoice::Builtin(ThemePreset::CatppuccinMocha) => Some(("#313244", "#cba6f7", "#94e2d5", "#cba6f7")),
                ThemeChoice::Custom(custom) => custom.yasb.as_ref().and_then(|y| {
                    match (
                        y.circle_background_color.as_deref(),
                        y.circle_work_progress_color.as_deref(),
                        y.circle_break_progress_color.as_deref(),
                    ) {
                        (Some(bg), Some(work), Some(brk)) => Some((
                            bg,
                            work,
                            brk,
                            y.current_line_color.as_deref().unwrap_or(work),
                        )),
                        _ => None,
                    }
                }),
            };
            if let Some((bg, work, brk, line)) = colors {
                let updated = update_yasb_config_colors(&cfg_raw, bg, work, brk, line);
                if updated != cfg_raw {
                    let _ = fs::write(&cfg_path, &updated);
                }
            }
        }
    }
}

fn apply_replacements(mut text: String, pairs: &[(&str, &str)]) -> String {
    for (from, to) in pairs {
        text = text.replace(from, to);
    }
    text
}

fn transform_css_tokyo_night(master: &str) -> String {
    apply_replacements(
        master.to_string(),
        &[
            ("--obsidian: #09080f;", "--obsidian: #16161e;"),
            ("--velvet-deep: #130f21;", "--velvet-deep: #1a1b26;"),
            ("--velvet-mid: #1d1633;", "--velvet-mid: #24283b;"),
            ("--velvet-soft: #2a2047;", "--velvet-soft: #292e42;"),
            ("--lilac-glow: #d6c2ff;", "--lilac-glow: #7aa2f7;"),
            ("--amethyst: #b79ced;", "--amethyst: #bb9af7;"),
            ("--iris: #9c7ce6;", "--iris: #3d59a1;"),
            ("--cerulean: #7ea8dc;", "--cerulean: #7dcfff;"),
            ("--cypress-teal: #58a89a;", "--cypress-teal: #73daca;"),
            ("--terracotta: #e07a6b;", "--terracotta: #f7768e;"),
            ("--moonlight: #f0ecf7;", "--moonlight: #c0caf5;"),
            ("--mist: #c0b8d4;", "--mist: #a9b1d6;"),
            ("--horizon: #746c8c;", "--horizon: #565f89;"),
            ("rgba(5, 5, 8, 0.90)", "rgba(22, 22, 30, 0.92)"),
            ("rgba(16, 12, 26, 0.88)", "rgba(26, 27, 38, 0.88)"),
            ("rgba(38, 28, 66, 0.88)", "rgba(41, 46, 66, 0.90)"),
            ("rgba(20, 15, 34, 0.92)", "rgba(26, 27, 38, 0.94)"),
            ("rgba(28, 20, 48, 0.94)", "rgba(36, 40, 59, 0.96)"),
            ("rgba(30, 22, 52, 0.96)", "rgba(41, 46, 66, 0.96)"),
            ("rgba(42, 30, 72, 0.96)", "rgba(52, 59, 88, 0.96)"),
            ("#19132c", "#1f2335"),
            ("#1a1330", "#1f2335"),
            ("#140f24", "#1a1b26"),
            ("#120e22", "#1a1b26"),
            ("#120d22", "#1a1b26"),
            ("#100d1b", "#1a1b26"),
            ("#0b0914", "#16161e"),
            ("#0a0812", "#16161e"),
            ("#07060c", "#16161e"),
            ("#241b3d", "#292e42"),
            ("#d6c2ff", "#7aa2f7"),
            ("#9c7ce6", "#bb9af7"),
            ("183, 156, 237", "122, 162, 247"),
            ("214, 194, 255", "187, 154, 247"),
            ("42, 32, 71", "41, 46, 66"),
            ("34, 24, 58", "41, 46, 66"),
            ("36, 26, 62", "41, 46, 66"),
            ("42, 30, 72", "41, 46, 66"),
            ("24, 18, 42", "31, 35, 53"),
            ("25, 19, 44", "31, 35, 53"),
            ("20, 15, 36", "31, 35, 53"),
            ("18, 13, 32", "22, 22, 30"),
            ("15, 11, 26", "22, 22, 30"),
            ("13, 10, 22", "22, 22, 30"),
            ("11, 9, 20", "22, 22, 30"),
            ("10, 8, 18", "22, 22, 30"),
            ("88, 168, 154", "115, 218, 202"),
            ("224, 122, 107", "247, 118, 142"),
            ("116, 108, 140", "86, 95, 137"),
        ],
    )
}

fn transform_css_catppuccin_mocha(master: &str) -> String {
    apply_replacements(
        master.to_string(),
        &[
            ("--obsidian: #09080f;", "--obsidian: #11111b;"),
            ("--velvet-deep: #130f21;", "--velvet-deep: #181825;"),
            ("--velvet-mid: #1d1633;", "--velvet-mid: #1e1e2e;"),
            ("--velvet-soft: #2a2047;", "--velvet-soft: #313244;"),
            ("--lilac-glow: #d6c2ff;", "--lilac-glow: #cba6f7;"),
            ("--amethyst: #b79ced;", "--amethyst: #b4befe;"),
            ("--iris: #9c7ce6;", "--iris: #89b4fa;"),
            ("--cerulean: #7ea8dc;", "--cerulean: #89b4fa;"),
            ("--cypress-teal: #58a89a;", "--cypress-teal: #94e2d5;"),
            ("--terracotta: #e07a6b;", "--terracotta: #f38ba8;"),
            ("--moonlight: #f0ecf7;", "--moonlight: #cdd6f4;"),
            ("--mist: #c0b8d4;", "--mist: #bac2de;"),
            ("--horizon: #746c8c;", "--horizon: #6c7086;"),
            ("rgba(5, 5, 8, 0.90)", "rgba(17, 17, 27, 0.92)"),
            ("rgba(16, 12, 26, 0.88)", "rgba(24, 24, 37, 0.88)"),
            ("rgba(38, 28, 66, 0.88)", "rgba(49, 50, 68, 0.90)"),
            ("rgba(20, 15, 34, 0.92)", "rgba(24, 24, 37, 0.94)"),
            ("rgba(28, 20, 48, 0.94)", "rgba(30, 30, 46, 0.96)"),
            ("rgba(30, 22, 52, 0.96)", "rgba(49, 50, 68, 0.96)"),
            ("rgba(42, 30, 72, 0.96)", "rgba(69, 71, 90, 0.96)"),
            ("#19132c", "#1e1e2e"),
            ("#1a1330", "#1e1e2e"),
            ("#140f24", "#181825"),
            ("#120e22", "#181825"),
            ("#120d22", "#181825"),
            ("#100d1b", "#181825"),
            ("#0b0914", "#11111b"),
            ("#0a0812", "#11111b"),
            ("#07060c", "#11111b"),
            ("#241b3d", "#313244"),
            ("#d6c2ff", "#cba6f7"),
            ("#9c7ce6", "#b4befe"),
            ("183, 156, 237", "203, 166, 247"),
            ("214, 194, 255", "180, 190, 254"),
            ("42, 32, 71", "49, 50, 68"),
            ("34, 24, 58", "49, 50, 68"),
            ("36, 26, 62", "49, 50, 68"),
            ("42, 30, 72", "49, 50, 68"),
            ("24, 18, 42", "30, 30, 46"),
            ("25, 19, 44", "30, 30, 46"),
            ("20, 15, 36", "30, 30, 46"),
            ("18, 13, 32", "17, 17, 27"),
            ("15, 11, 26", "17, 17, 27"),
            ("13, 10, 22", "17, 17, 27"),
            ("11, 9, 20", "17, 17, 27"),
            ("10, 8, 18", "17, 17, 27"),
            ("88, 168, 154", "148, 226, 213"),
            ("224, 122, 107", "243, 139, 168"),
            ("116, 108, 140", "108, 112, 134"),
        ],
    )
}

fn transform_css_carbon_dark(master: &str) -> String {
    apply_replacements(
        master.to_string(),
        &[
            ("--obsidian: #09080f;", "--obsidian: #08080a;"),
            ("--velvet-deep: #130f21;", "--velvet-deep: #0b0b0e;"),
            ("--velvet-mid: #1d1633;", "--velvet-mid: #18181d;"),
            ("--velvet-soft: #2a2047;", "--velvet-soft: #24242b;"),
            ("--lilac-glow: #d6c2ff;", "--lilac-glow: #e4e4e7;"),
            ("--amethyst: #b79ced;", "--amethyst: #a1a1aa;"),
            ("--iris: #9c7ce6;", "--iris: #71717a;"),
            ("--cerulean: #7ea8dc;", "--cerulean: #7dd3fc;"),
            ("--cypress-teal: #58a89a;", "--cypress-teal: #6ee7b7;"),
            ("--terracotta: #e07a6b;", "--terracotta: #fca5a5;"),
            ("--moonlight: #f0ecf7;", "--moonlight: #f4f4f5;"),
            ("--mist: #c0b8d4;", "--mist: #a1a1aa;"),
            ("--horizon: #746c8c;", "--horizon: #71717a;"),
            ("rgba(5, 5, 8, 0.90)", "rgba(8, 8, 10, 0.92)"),
            ("rgba(16, 12, 26, 0.88)", "rgba(11, 11, 14, 0.90)"),
            ("rgba(38, 28, 66, 0.88)", "rgba(36, 36, 43, 0.90)"),
            ("rgba(20, 15, 34, 0.92)", "rgba(14, 14, 18, 0.94)"),
            ("rgba(28, 20, 48, 0.94)", "rgba(24, 24, 29, 0.96)"),
            ("rgba(30, 22, 52, 0.96)", "rgba(36, 36, 43, 0.96)"),
            ("rgba(42, 30, 72, 0.96)", "rgba(48, 48, 56, 0.96)"),
            ("#19132c", "#18181d"),
            ("#1a1330", "#18181d"),
            ("#140f24", "#0b0b0e"),
            ("#120e22", "#0b0b0e"),
            ("#120d22", "#0b0b0e"),
            ("#100d1b", "#0b0b0e"),
            ("#0b0914", "#08080a"),
            ("#0a0812", "#08080a"),
            ("#07060c", "#08080a"),
            ("#241b3d", "#24242b"),
            ("#d6c2ff", "#e4e4e7"),
            ("#9c7ce6", "#a1a1aa"),
            ("183, 156, 237", "228, 228, 231"),
            ("214, 194, 255", "244, 244, 245"),
            ("42, 32, 71", "36, 36, 43"),
            ("34, 24, 58", "36, 36, 43"),
            ("36, 26, 62", "36, 36, 43"),
            ("42, 30, 72", "36, 36, 43"),
            ("24, 18, 42", "24, 24, 29"),
            ("25, 19, 44", "24, 24, 29"),
            ("20, 15, 36", "24, 24, 29"),
            ("18, 13, 32", "11, 11, 14"),
            ("15, 11, 26", "11, 11, 14"),
            ("13, 10, 22", "11, 11, 14"),
            ("11, 9, 20", "11, 11, 14"),
            ("10, 8, 18", "11, 11, 14"),
            ("88, 168, 154", "110, 231, 183"),
            ("224, 122, 107", "252, 165, 165"),
            ("116, 108, 140", "113, 113, 122"),
        ],
    )
}

fn update_yasb_config_colors(
    cfg: &str,
    bg: &str,
    work: &str,
    brk: &str,
    line_color: &str,
) -> String {

    let mut out = String::with_capacity(cfg.len());
    for line in cfg.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let newline = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };

        if trimmed.starts_with("circle_background_color:") {
            out.push_str(&format!("{indent}circle_background_color: \"{bg}\"{newline}"));
        } else if trimmed.starts_with("circle_work_progress_color:") {
            out.push_str(&format!(
                "{indent}circle_work_progress_color: \"{work}\"{newline}"
            ));
        } else if trimmed.starts_with("circle_break_progress_color:") {
            out.push_str(&format!(
                "{indent}circle_break_progress_color: \"{brk}\"{newline}"
            ));
        } else if trimmed.starts_with("current_line_color:") {
            out.push_str(&format!("{indent}current_line_color: \"{line_color}\"{newline}"));
        } else {
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_resolve_and_preserve_fonts() {
        let mut base = ThemeConfig::default();
        base.font_family = "Inter".to_string();
        base.mono_font_family = "Fira Code".to_string();
        base.font_size = 15.0;
        base.sync_yasb = false;

        for preset in ThemePreset::all() {
            let applied = preset.apply_to(&base);
            assert_eq!(applied.font_family, "Inter");
            assert_eq!(applied.mono_font_family, "Fira Code");
            assert_eq!(applied.font_size, 15.0);
            assert!(!applied.sync_yasb);
            let resolved = applied.resolve();
            assert!(resolved.background.a > 0.5);
            assert!(resolved.accent.a > 0.9);
            assert_eq!(
                ThemePreset::from_slug(preset.slug()),
                Some(*preset)
            );
        }
    }

    #[test]
    fn yasb_config_line_updater_preserves_other_lines() {
        let sample = "  pomodoro:\n    circle_background_color: \"#2a2047\"\n    circle_work_progress_color: \"#d6c2ff\"\n    circle_break_progress_color: \"#58a89a\"\n  weather:\n    current_line_color: \"#d6c2ff\"\n";
        let updated = update_yasb_config_colors(sample, "#292e42", "#7aa2f7", "#73daca", "#7aa2f7");
        assert!(updated.contains("circle_background_color: \"#292e42\""));
        assert!(updated.contains("circle_work_progress_color: \"#7aa2f7\""));
        assert!(updated.contains("circle_break_progress_color: \"#73daca\""));
        assert!(updated.contains("current_line_color: \"#7aa2f7\""));
    }

    #[test]
    fn detect_from_config_matches_all_presets_and_ignores_custom() {
        let base = ThemeConfig::default();
        for preset in ThemePreset::all() {
            let cfg = preset.apply_to(&base);
            assert_eq!(
                detect_active_theme_slug(&cfg).as_deref(),
                Some(preset.slug())
            );
        }

        let mut custom = base.clone();
        custom.background = "#123456ff".to_string();
        custom.accent = "#fedcbaff".to_string();
        assert_eq!(detect_active_theme_slug(&custom), None);
    }

    #[test]
    fn parses_custom_theme_with_wrapper_and_raw_sections() {
        let wrapped = r##"
name = "Midnight Velvet"
[theme]
background = "#101018f6"
accent = "#ff79c6ff"
[yasb]
restore_master_css = true
circle_background_color = "#1f1f2e"
circle_work_progress_color = "#ff79c6"
circle_break_progress_color = "#50fa7b"
"##;
        let parsed = parse_custom_theme_str("midnight", wrapped).expect("wrapped custom theme");
        assert_eq!(parsed.name, "Midnight Velvet");
        assert_eq!(parsed.config.background, "#101018f6");
        assert_eq!(parsed.config.accent, "#ff79c6ff");
        assert!(parsed.yasb.as_ref().unwrap().restore_master_css);

        let raw = r##"
background = "#09090df6"
accent = "#aabbccff"
"##;
        let parsed_raw = parse_custom_theme_str("slate-mist", raw).expect("raw custom theme");
        assert_eq!(parsed_raw.name, "Slate Mist");
        assert_eq!(parsed_raw.config.accent, "#aabbccff");

        assert!(parse_custom_theme_str("carbon-dark", raw).is_none());
    }
}
