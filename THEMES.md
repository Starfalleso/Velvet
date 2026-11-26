# Velvet Theme Presets

- **Instant Live Switch (Recommended):** Open Velvet (`Win` or `Alt+Space`), type `theme`, and press `Enter` on **Carbon Dark**, **Tokyo Night**, or **Catppuccin Mocha** — Velvet updates immediately (and can sync YASB live when `sync_yasb = true`).
- **Tray Menu Switch:** Right-click the **Velvet** tray icon → **Theme ▸** and pick a preset (shows a checkmark on the active preset).
- **CLI Switch:** `velvet.exe --theme carbon-dark` (or `tokyo-night`, `catppuccin-mocha`).
- **Manual Edit:** Copy any `[theme]` block below into `%APPDATA%\velvet\config.toml` and save; Velvet hot-reloads it within ~750 ms.
- **Drop-In Custom Themes:** Save any `.toml` file (containing a `[theme]` block or direct color keys, plus optional `name = "..."`) into `%APPDATA%\velvet\themes\` (or `themes\` next to `velvet.exe`). It automatically appears in the tray **Theme ▸** menu, launcher search (`theme`), and `--theme <filename>`.

---

## 1. Carbon Dark (Default)

Neutral obsidian-carbon dark theme with subtle translucent borders and crisp silver-white highlights.

```toml
[theme]
font_family = "Poppins"
mono_font_family = "JetBrainsMono NFP"
font_size = 13.5
background = "#0b0b0ef6"
surface = "#08080af4"
surface_focused = "#0b0b0ef6"
row_hover = "#1a1a20cc"
row_selected = "#24242be8"
border = "#ffffff14"
border_subtle = "#ffffff0c"
accent = "#e4e4e7ff"
accent_secondary = "#a1a1aaff"
text_primary = "#f4f4f5ff"
text_secondary = "#a1a1aaff"
text_muted = "#71717aff"
badge_app = "#e4e4e722"
badge_app_text = "#e4e4e7ff"
badge_store = "#7dd3fc22"
badge_store_text = "#7dd3fcff"
badge_math = "#6ee7b722"
badge_math_text = "#6ee7b7ff"
badge_system = "#fca5a522"
badge_system_text = "#fca5a5ff"
```

---

## 2. Tokyo Night (Night)

Crisp midnight navy (`#1a1b26`) with storm slate selection, electric blue (`#7aa2f7`), lavender (`#bb9af7`), and cyan/mint/coral accents.

```toml
[theme]
font_family = "Poppins"
mono_font_family = "JetBrainsMono NFP"
font_size = 13.5
background = "#1a1b26f6"
surface = "#16161ef4"
surface_focused = "#1a1b26f6"
row_hover = "#24283bcc"
row_selected = "#292e42e8"
border = "#7aa2f71c"
border_subtle = "#41486838"
accent = "#7aa2f7ff"
accent_secondary = "#bb9af7ff"
text_primary = "#c0caf5ff"
text_secondary = "#a9b1d6ff"
text_muted = "#565f89ff"
badge_app = "#7aa2f722"
badge_app_text = "#7aa2f7ff"
badge_store = "#7dcfff22"
badge_store_text = "#7dcfffff"
badge_math = "#73daca22"
badge_math_text = "#73dacaff"
badge_system = "#f7768e22"
badge_system_text = "#f7768eff"
```

---

## 3. Catppuccin Mocha

Warm espresso-crust dark theme with mauve/lavender highlights.

```toml
[theme]
font_family = "Poppins"
mono_font_family = "JetBrainsMono NFP"
font_size = 13.5
background = "#11111bf6"
surface = "#181825f2"
surface_focused = "#11111bf6"
row_hover = "#31324499"
row_selected = "#313244e6"
border = "#45475a38"
border_subtle = "#31324455"
accent = "#cba6f7ff"
accent_secondary = "#b4befeff"
text_primary = "#cdd6f4ff"
text_secondary = "#bac2deff"
text_muted = "#6c7086ff"
badge_app = "#cba6f722"
badge_app_text = "#cba6f7ff"
badge_store = "#89b4fa22"
badge_store_text = "#89b4faff"
badge_math = "#94e2d522"
badge_math_text = "#94e2d5ff"
badge_system = "#f38ba822"
badge_system_text = "#f38ba8ff"
```
