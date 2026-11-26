//! Safe YASB bar widget wiring for `velvet.exe --setup-yasb`.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YasbSetupStatus {
    Configured,
    UpdatedPath,
    AlreadyUpToDate,
    YasbNotInstalled,
}

fn yaml_escape_path(exe_path: &Path) -> String {
    exe_path.to_string_lossy().replace('\\', "\\\\")
}

fn detect_newline(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Replaces any `on_left:` / `on_right:` command referencing `velvet.exe` with `new_exe`,
/// preserving existing flags (such as `--toggle` / `--toggle-taskbar`).
fn rewrite_existing_velvet_callbacks(cfg: &str, escaped_exe: &str) -> (String, bool) {
    let mut changed = false;
    let mut out = String::with_capacity(cfg.len() + 64);

    for raw_line in cfg.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let line_nl = if raw_line.ends_with("\r\n") {
            "\r\n"
        } else if raw_line.ends_with('\n') {
            "\n"
        } else {
            ""
        };

        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        let is_callback = lower.starts_with("on_left:") || lower.starts_with("on_right:");

        if is_callback && lower.contains("velvet.exe") {
            let indent = &line[..line.len() - trimmed.len()];
            let (key, rest) = trimmed.split_once(':').unwrap_or((trimmed, ""));
            let tail_flag = if lower.contains("--toggle-taskbar") {
                "--toggle-taskbar"
            } else if lower.contains("--hide-taskbar") {
                "--hide-taskbar"
            } else if lower.contains("--show-taskbar") {
                "--show-taskbar"
            } else if lower.contains("--show") {
                "--show"
            } else {
                "--toggle"
            };
            let new_line = format!("{indent}{key}: \"exec {escaped_exe} {tail_flag}\"");
            if new_line != line && !rest.is_empty() {
                changed = true;
            }
            out.push_str(&new_line);
            out.push_str(line_nl);
        } else {
            out.push_str(line);
            out.push_str(line_nl);
        }
    }

    (out, changed)
}

fn has_widget_entry(lines: &[&str], widget_name: &str) -> bool {
    let needle = format!("\"{widget_name}\"");
    let single = format!("'{widget_name}'");
    lines.iter().any(|l| {
        let t = l.trim();
        t == format!("- {needle}") || t == format!("- {single}") || t == format!("- {widget_name}")
    })
}

fn insert_bar_widget_and_definition(cfg: &str, escaped_exe: &str) -> Option<String> {
    let nl = detect_newline(cfg);
    let mut lines: Vec<String> = cfg.lines().map(ToString::to_string).collect();
    if lines.is_empty() {
        return None;
    }

    // 1. Insert `- "velvet"` into the first bar's `left:` list if not already present.
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let already_on_bar = has_widget_entry(&line_refs, "velvet");
    if !already_on_bar {
        let mut in_bars = false;
        let mut in_bar_widgets = false;
        let mut left_idx: Option<usize> = None;
        let mut left_indent = String::from("        ");
        let mut home_idx: Option<usize> = None;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();
            if indent == 0 && trimmed.starts_with("bars:") {
                in_bars = true;
                continue;
            }
            if in_bars && indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
                break;
            }
            if in_bars && trimmed == "widgets:" {
                in_bar_widgets = true;
                continue;
            }
            if in_bar_widgets && trimmed == "left:" {
                left_idx = Some(i);
                continue;
            }
            if let Some(li) = left_idx {
                let left_base_indent = lines[li].len() - lines[li].trim_start().len();
                if !trimmed.is_empty() && !trimmed.starts_with('#') && indent <= left_base_indent {
                    break;
                }
                if trimmed.starts_with('-') {
                    left_indent = line[..indent].to_string();
                    if trimmed.contains("\"home\"") || trimmed.contains("'home'") {
                        home_idx = Some(i);
                    }
                }
            }
        }

        if let Some(li) = left_idx {
            let insert_at = home_idx.unwrap_or(li) + 1;
            lines.insert(insert_at, format!("{left_indent}- \"velvet\""));
        }
    }

    // 2. Insert `velvet:` definition under top-level `widgets:` (if `velvet:` not already defined).
    let has_velvet_def = lines.iter().any(|l| {
        let t = l.trim();
        let indent = l.len() - l.trim_start().len();
        indent == 2 && t == "velvet:"
    });

    if !has_velvet_def {
        let widgets_idx = lines
            .iter()
            .position(|l| l.len() == l.trim_start().len() && l.trim() == "widgets:")?;

        let block = [
            "  velvet:".to_string(),
            "    type: \"yasb.custom.CustomWidget\"".to_string(),
            "    options:".to_string(),
            "      label: \"<span>\\udb80\\udd3b</span>\"".to_string(),
            "      label_alt: \"<span>\\udb80\\udd3b</span>\"".to_string(),
            "      class_name: \"velvet-widget\"".to_string(),
            "      tooltip: true".to_string(),
            "      tooltip_label: \"Velvet (Left: Toggle | Right: Toggle Taskbar)\"".to_string(),
            "      callbacks:".to_string(),
            format!("        on_left: \"exec {escaped_exe} --toggle\""),
            "        on_middle: \"do_nothing\"".to_string(),
            format!("        on_right: \"exec {escaped_exe} --toggle-taskbar\""),
        ];

        for (offset, entry) in block.into_iter().enumerate() {
            lines.insert(widgets_idx + 1 + offset, entry);
        }
    }

    let mut out = lines.join(nl);
    if cfg.ends_with('\n') {
        out.push_str(nl);
    }
    Some(out)
}

/// Applies safe YASB `config.yaml` wiring in-memory (pure function for unit testing).
pub fn apply_yasb_config_setup(cfg: &str, exe_path: &Path) -> (String, YasbSetupStatus) {
    let escaped = yaml_escape_path(exe_path);

    if cfg.to_ascii_lowercase().contains("velvet.exe") {
        let (updated, changed) = rewrite_existing_velvet_callbacks(cfg, &escaped);
        let status = if changed {
            YasbSetupStatus::UpdatedPath
        } else {
            YasbSetupStatus::AlreadyUpToDate
        };
        return (updated, status);
    }

    if let Some(updated) = insert_bar_widget_and_definition(cfg, &escaped) {
        return (updated, YasbSetupStatus::Configured);
    }

    (cfg.to_string(), YasbSetupStatus::YasbNotInstalled)
}

/// Configures `%USERPROFILE%\.config\yasb\config.yaml` (backing up to `config.yaml.velvet.bak`
/// first) and adds a minimal `.velvet-widget` rule to `styles.css` if needed.
pub fn run_yasb_setup(exe_path: &Path) -> anyhow::Result<YasbSetupStatus> {
    let Some(home) = dirs::home_dir() else {
        return Ok(YasbSetupStatus::YasbNotInstalled);
    };
    let yasb_dir = home.join(".config").join("yasb");
    let cfg_path = yasb_dir.join("config.yaml");
    if !cfg_path.exists() {
        return Ok(YasbSetupStatus::YasbNotInstalled);
    }

    let raw = fs::read_to_string(&cfg_path)?;
    let (updated, status) = apply_yasb_config_setup(&raw, exe_path);

    if updated != raw {
        let backup_path = yasb_dir.join("config.yaml.velvet.bak");
        if !backup_path.exists() {
            let _ = fs::copy(&cfg_path, &backup_path);
        }
        fs::write(&cfg_path, &updated)?;
    }

    // Ensure `.velvet-widget` has clean padding if `styles.css` exists and doesn't style it yet.
    let css_path = yasb_dir.join("styles.css");
    if status == YasbSetupStatus::Configured && css_path.exists() {
        if let Ok(css_raw) = fs::read_to_string(&css_path) {
            if !css_raw.contains(".velvet-widget") && !css_raw.contains(".launchpad-widget") {
                let nl = detect_newline(&css_raw);
                let snippet = format!(
                    "{nl}/* Velvet Launcher Button */{nl}\
                     .velvet-widget {{{nl}\
                     {sp}padding: 0 8px;{nl}\
                     }}{nl}",
                    sp = "    "
                );
                let mut new_css = css_raw;
                if !new_css.ends_with('\n') {
                    new_css.push_str(nl);
                }
                new_css.push_str(&snippet);
                let _ = fs::write(&css_path, new_css);
            }
        }
    }

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_fresh_yasb_yaml_without_clobbering_other_widgets() {
        let sample = "\
bars:
  primary-bar:
    widgets:
      left:
        - \"home\"
        - \"glazewm_workspaces\"
      right:
        - \"clock\"

widgets:
  home:
    type: \"yasb.home.HomeWidget\"
";
        let exe = Path::new(r"C:\Users\alice\AppData\Local\Programs\Velvet\velvet.exe");
        let (out, status) = apply_yasb_config_setup(sample, exe);
        assert_eq!(status, YasbSetupStatus::Configured);
        assert!(out.contains("        - \"home\"\n        - \"velvet\"\n"));
        assert!(out.contains("  velvet:\n    type: \"yasb.custom.CustomWidget\""));
        assert!(out.contains(
            r#"on_left: "exec C:\\Users\\alice\\AppData\\Local\\Programs\\Velvet\\velvet.exe --toggle""#
        ));
        assert!(out.contains(
            r#"on_right: "exec C:\\Users\\alice\\AppData\\Local\\Programs\\Velvet\\velvet.exe --toggle-taskbar""#
        ));

        // Second pass updates cleanly without adding duplicates.
        let (out2, status2) = apply_yasb_config_setup(&out, exe);
        assert_eq!(status2, YasbSetupStatus::AlreadyUpToDate);
        assert_eq!(out2, out);
    }

    #[test]
    fn updates_existing_launchpad_callbacks_in_place() {
        let sample = "\
bars:
  primary-bar:
    widgets:
      left:
        - \"home\"
        - \"launchpad\"

widgets:
  launchpad:
    type: \"yasb.custom.CustomWidget\"
    options:
      callbacks:
        on_left: \"exec C:\\\\old\\\\velvet.exe --toggle\"
        on_middle: \"do_nothing\"
        on_right: \"exec C:\\\\old\\\\velvet.exe --toggle-taskbar\"
";
        let exe = Path::new(r"C:\Users\bob\AppData\Local\Programs\Velvet\velvet.exe");
        let (out, status) = apply_yasb_config_setup(sample, exe);
        assert_eq!(status, YasbSetupStatus::UpdatedPath);
        assert!(!out.contains(r"C:\\old\\velvet.exe"));
        assert!(out.contains(
            r#"on_left: "exec C:\\Users\\bob\\AppData\\Local\\Programs\\Velvet\\velvet.exe --toggle""#
        ));
        assert!(out.contains(
            r#"on_right: "exec C:\\Users\\bob\\AppData\\Local\\Programs\\Velvet\\velvet.exe --toggle-taskbar""#
        ));
        // Should not add a second `velvet` widget when `launchpad` already hosts Velvet.
        assert!(!out.contains("  velvet:"));
    }
}
