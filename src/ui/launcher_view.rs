use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::Sender;
use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, uniform_list, BoxShadow,
    ClipboardItem, Context, FocusHandle, Focusable, FontWeight, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba, ScrollStrategy, SharedString,
    Task, TextRun, UniformListScrollHandle, Window,
};

use crate::commands::AppCommand;
use crate::config::{self, Config, ResolvedTheme};
use crate::indexer::{self, AppCategory, IndexedApp, SystemActionKind};
use crate::search::{SearchEngine, SearchItem, SearchResult};
use crate::taskbar;
use crate::themes::{self, ThemeChoice};
use crate::usage::UsageStore;
use crate::win_hooks;
use crate::win_window;

/// Exact window-relative X coordinate where search query text begins
/// (`WINDOW_PAD_X` + 1px border + 19px padding + 22px icon + 10px gap).
const INPUT_TEXT_X: f32 = win_window::WINDOW_PAD_X + 1.0 + 19.0 + 22.0 + 10.0;

#[derive(Debug, Clone, Copy)]
enum AnimState {
    Idle,
    Opening(Instant),
    Closing(Instant),
}

pub struct LauncherView {
    config: Config,
    config_path: PathBuf,
    theme: ResolvedTheme,
    apps: Arc<Vec<Arc<IndexedApp>>>,
    usage: UsageStore,
    search_engine: SearchEngine,
    query: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    selecting_text: bool,
    input_scroll_x: f32,
    results: Vec<SearchResult>,
    selected: usize,
    is_visible: bool,
    anim: AnimState,
    last_shown_at: Option<Instant>,
    hwnd: Option<isize>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    cmd_tx: Sender<AppCommand>,
    indexing: bool,
    status_hint: Option<(String, Instant)>,
    pending_power_confirm: Option<(SystemActionKind, Instant)>,
    caret_visible: bool,
    caret_epoch: u64,
    _blink_task: Option<Task<()>>,
}

impl LauncherView {
    pub fn new(
        config: Config,
        config_path: PathBuf,
        hwnd: Option<isize>,
        cmd_tx: Sender<AppCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = config.theme.resolve();
        let usage = UsageStore::load_default();
        let focus_handle = cx.focus_handle();

        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.is_visible && this.config.window.hide_on_blur {
                let elapsed = this
                    .last_shown_at
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::from_secs(10));
                if elapsed > Duration::from_millis(200) {
                    this.hide(cx);
                }
            }
        })
        .detach();

        let mut view = Self {
            config,
            config_path,
            theme,
            apps: Arc::new(Vec::new()),
            usage,
            search_engine: SearchEngine::new(),
            query: String::new(),
            cursor: 0,
            selection_anchor: None,
            selecting_text: false,
            input_scroll_x: 0.0,
            results: Vec::new(),
            selected: 0,
            is_visible: false,
            anim: AnimState::Idle,
            last_shown_at: None,
            hwnd,
            focus_handle,
            scroll_handle: UniformListScrollHandle::new(),
            cmd_tx,
            indexing: true,
            status_hint: None,
            pending_power_confirm: None,
            caret_visible: true,
            caret_epoch: 0,
            _blink_task: None,
        };
        view.recompute_results();
        view
    }

    pub fn set_apps(&mut self, apps: Vec<IndexedApp>, cx: &mut Context<Self>) {
        self.apps = Arc::new(apps.into_iter().map(Arc::new).collect());
        self.indexing = false;
        self.recompute_results();
        cx.notify();
    }

    pub fn set_indexing(&mut self, indexing: bool, cx: &mut Context<Self>) {
        self.indexing = indexing;
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_visible && !matches!(self.anim, AnimState::Closing(_)) {
            self.hide(cx);
        } else {
            self.show(window, cx);
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.selecting_text = false;
        self.input_scroll_x = 0.0;
        self.pending_power_confirm = None;
        self.selected = 0;
        self.status_hint = None;
        self.recompute_results();
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);

        self.is_visible = true;
        self.anim = if self.config.window.animation_ms > 0 {
            AnimState::Opening(Instant::now())
        } else {
            AnimState::Idle
        };
        self.last_shown_at = Some(Instant::now());
        win_hooks::set_launcher_visible(true);

        if let Some(hwnd) = self.hwnd {
            win_window::center_and_show(
                hwnd,
                self.config.window.width,
                self.config.window.max_height,
                self.config.window.top_offset_ratio,
            );
        }

        window.activate_window();
        window.focus(&self.focus_handle);
        self.reset_caret_blink(cx);
        window.refresh();
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        if !self.is_visible || matches!(self.anim, AnimState::Closing(_)) {
            return;
        }
        self.pending_power_confirm = None;
        win_hooks::set_launcher_visible(false);
        self.caret_epoch = self.caret_epoch.wrapping_add(1);
        self._blink_task = None;
        if self.config.window.animation_ms == 0 {
            self.hide_immediate(cx);
            return;
        }
        self.anim = AnimState::Closing(Instant::now());
        cx.notify();
    }

    pub fn hide_immediate(&mut self, cx: &mut Context<Self>) {
        win_hooks::set_launcher_visible(false);
        self.is_visible = false;
        self.selecting_text = false;
        self.selection_anchor = None;
        self.input_scroll_x = 0.0;
        self.pending_power_confirm = None;
        self.anim = AnimState::Idle;
        self.caret_epoch = self.caret_epoch.wrapping_add(1);
        self._blink_task = None;
        if let Some(hwnd) = self.hwnd {
            win_window::hide_hwnd(hwnd);
        }
        cx.notify();
    }

    fn reset_caret_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_visible = true;
        self.caret_epoch = self.caret_epoch.wrapping_add(1);
        let epoch = self.caret_epoch;
        self._blink_task = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(530))
                .await;
            let keep_going = this
                .update(cx, |view, cx| {
                    if !view.is_visible || view.caret_epoch != epoch {
                        return false;
                    }
                    view.caret_visible = !view.caret_visible;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !keep_going {
                break;
            }
        }));
    }

    pub fn refresh_taskbar_state(&mut self, cx: &mut Context<Self>) {
        self.recompute_results();
        cx.notify();
    }

    pub fn apply_config(&mut self, cfg: Config, cx: &mut Context<Self>) {
        self.theme = cfg.theme.resolve();
        win_hooks::set_active_theme_from_config(&cfg.theme);
        self.config = cfg;
        self.recompute_results();
        cx.notify();
    }

    pub fn apply_theme_preset(&mut self, choice: ThemeChoice, cx: &mut Context<Self>) {
        self.config.theme = choice.apply_to(&self.config.theme);
        self.theme = self.config.theme.resolve();
        win_hooks::set_active_theme_from_config(&self.config.theme);
        self.status_hint = Some((format!("Theme: {}", choice.name()), Instant::now()));

        let cfg_clone = self.config.clone();
        let cfg_path = self.config_path.clone();
        let sync_yasb = self.config.theme.sync_yasb;
        std::thread::spawn(move || {
            let _ = config::save_config(&cfg_path, &cfg_clone);
            if sync_yasb {
                themes::sync_yasb_theme(&choice);
            }
        });
        cx.notify();
    }

    pub fn toggle_win_key_hook(&mut self, cx: &mut Context<Self>) -> bool {
        let enabled = !self.config.hotkeys.intercept_win_key;
        self.config.hotkeys.intercept_win_key = enabled;
        self.status_hint = Some((
            if enabled {
                "Windows Key: On".to_string()
            } else {
                "Windows Key: Off".to_string()
            },
            Instant::now(),
        ));

        let cfg_clone = self.config.clone();
        let cfg_path = self.config_path.clone();
        std::thread::spawn(move || {
            let _ = config::save_config(&cfg_path, &cfg_clone);
        });

        self.recompute_results();
        cx.notify();
        enabled
    }

    pub fn set_status_hint(&mut self, msg: String, cx: &mut Context<Self>) {
        self.status_hint = Some((msg, Instant::now()));
        cx.notify();
    }

    fn recompute_results(&mut self) {
        self.results = self.search_engine.search(
            &self.query,
            &self.apps,
            &self.usage,
            &self.config.search,
            taskbar::is_hidden(),
            self.config.hotkeys.intercept_win_key,
        );
        if self.results.is_empty() || self.selected >= self.results.len() {
            self.selected = 0;
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.pending_power_confirm = None;
        let len = self.results.len();
        if len == 0 {
            return;
        }
        let next = ((self.selected as isize + delta).rem_euclid(len as isize)) as usize;
        self.selected = next;
        let strategy = if delta < 0 {
            ScrollStrategy::Top
        } else {
            ScrollStrategy::Bottom
        };
        self.scroll_handle.scroll_to_item(self.selected, strategy);
        cx.notify();
    }

    fn activate_index(&mut self, idx: usize, as_admin: bool, cx: &mut Context<Self>) {
        let Some(result) = self.results.get(idx).cloned() else {
            return;
        };

        match result.item {
            SearchItem::Math(m) => {
                self.pending_power_confirm = None;
                cx.write_to_clipboard(ClipboardItem::new_string(m.formatted.clone()));
                self.status_hint = Some((format!("Copied {}", m.formatted), Instant::now()));
                self.hide_immediate(cx);
            }
            SearchItem::Command(cmd) => {
                self.pending_power_confirm = None;
                let line = cmd.command.clone();
                self.hide_immediate(cx);
                std::thread::spawn(move || {
                    indexer::run_shell_command(&line, as_admin);
                });
            }
            SearchItem::App(app) => {
                self.pending_power_confirm = None;
                self.usage.record_launch(&app.id);
                let target = app.launch.clone();
                self.hide_immediate(cx);
                std::thread::spawn(move || {
                    indexer::launch_app(&target, as_admin);
                });
            }
            SearchItem::System(sys) => match sys.kind.clone() {
                SystemActionKind::ReloadApps => {
                    self.pending_power_confirm = None;
                    self.indexing = true;
                    let _ = self.cmd_tx.try_send(AppCommand::ReloadApps);
                    self.hide_immediate(cx);
                }
                SystemActionKind::ToggleTaskbar => {
                    self.pending_power_confirm = None;
                    let _ = self.cmd_tx.try_send(AppCommand::ToggleTaskbar);
                    self.hide_immediate(cx);
                }
                SystemActionKind::ToggleWinKeyHook => {
                    self.pending_power_confirm = None;
                    let _ = self.cmd_tx.try_send(AppCommand::ToggleWinKeyHook);
                }
                SystemActionKind::OpenConfig => {
                    self.pending_power_confirm = None;
                    let path = self.config_path.clone();
                    self.hide_immediate(cx);
                    std::thread::spawn(move || {
                        indexer::open_file_in_shell(&path);
                    });
                }
                SystemActionKind::CheckUpdates => {
                    self.pending_power_confirm = None;
                    self.status_hint = Some(("Checking for updates…".to_string(), Instant::now()));
                    let _ = self.cmd_tx.try_send(AppCommand::CheckForUpdates {
                        manual: true,
                        notify_tray: false,
                    });
                    cx.notify();
                }
                SystemActionKind::InstallUpdate => {
                    self.pending_power_confirm = None;
                    self.status_hint = Some(("Downloading update…".to_string(), Instant::now()));
                    let _ = self.cmd_tx.try_send(AppCommand::InstallUpdate);
                    cx.notify();
                }
                SystemActionKind::SetTheme(preset) => {
                    self.pending_power_confirm = None;
                    self.apply_theme_preset(preset, cx);
                }
                power_kind @ (SystemActionKind::Restart | SystemActionKind::Shutdown) => {
                    let confirmed = self
                        .pending_power_confirm
                        .as_ref()
                        .map(|(k, at)| *k == power_kind && at.elapsed() < Duration::from_secs(4))
                        .unwrap_or(false);
                    if confirmed {
                        self.pending_power_confirm = None;
                        self.hide_immediate(cx);
                        std::thread::spawn(move || {
                            indexer::execute_power_action(power_kind);
                        });
                    } else {
                        self.pending_power_confirm = Some((power_kind, Instant::now()));
                        self.status_hint = Some((
                            format!("Press ↵ again to {}", sys.name),
                            Instant::now(),
                        ));
                        cx.notify();
                    }
                }
                power_kind => {
                    self.pending_power_confirm = None;
                    self.hide_immediate(cx);
                    std::thread::spawn(move || {
                        indexer::execute_power_action(power_kind);
                    });
                }
            },
        }
    }

    fn byte_index_at_char(&self, char_idx: usize) -> usize {
        self.query
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.query.len())
    }

    fn char_len(&self) -> usize {
        self.query.chars().count()
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            None
        } else {
            Some((anchor.min(self.cursor), anchor.max(self.cursor)))
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        let s = self.byte_index_at_char(start);
        let e = self.byte_index_at_char(end);
        Some(self.query[s..e].to_string())
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let s = self.byte_index_at_char(start);
        let e = self.byte_index_at_char(end);
        self.query.replace_range(s..e, "");
        self.cursor = start;
        self.selection_anchor = None;
        self.selected = 0;
        self.recompute_results();
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        true
    }

    fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.char_len();
    }

    fn prev_word_boundary(&self, from: usize) -> usize {
        let chars: Vec<char> = self.query.chars().collect();
        let mut i = from.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn next_word_boundary(&self, from: usize) -> usize {
        let chars: Vec<char> = self.query.chars().collect();
        let mut i = from.min(chars.len());
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn select_word_at(&mut self, idx: usize) {
        let chars: Vec<char> = self.query.chars().collect();
        if chars.is_empty() {
            self.selection_anchor = None;
            self.cursor = 0;
            return;
        }
        let pos = idx.min(chars.len().saturating_sub(1));
        let classify = |c: char| -> u8 {
            if c.is_alphanumeric() || c == '_' {
                1
            } else if c.is_whitespace() {
                2
            } else {
                0
            }
        };
        let cls = classify(chars[pos]);
        let mut start = pos;
        while start > 0 && classify(chars[start - 1]) == cls {
            start -= 1;
        }
        let mut end = pos + 1;
        while end < chars.len() && classify(chars[end]) == cls {
            end += 1;
        }
        self.selection_anchor = Some(start);
        self.cursor = end;
    }

    fn char_index_for_window_x(&self, window_x: Pixels, window: &Window) -> usize {
        if self.query.is_empty() {
            return 0;
        }
        let local_x = (f32::from(window_x) - INPUT_TEXT_X + self.input_scroll_x).max(0.0);
        let run = TextRun {
            len: self.query.len(),
            font: gpui::font(self.theme.font_family.clone()),
            color: self.theme.text_primary,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            self.query.clone().into(),
            px(self.theme.font_size + 1.5),
            &[run],
            None,
        );
        let byte_idx = shaped.closest_index_for_x(px(local_x)).min(self.query.len());
        self.query[..byte_idx].chars().count()
    }

    fn sync_input_scroll(&mut self, window: &Window) {
        if self.query.is_empty() {
            self.input_scroll_x = 0.0;
            return;
        }
        let run = TextRun {
            len: self.query.len(),
            font: gpui::font(self.theme.font_family.clone()),
            color: self.theme.text_primary,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            self.query.clone().into(),
            px(self.theme.font_size + 1.5),
            &[run],
            None,
        );
        let caret_byte = self.byte_index_at_char(self.cursor);
        let caret_x = f32::from(shaped.x_for_index(caret_byte));
        let available_w = (self.config.window.width - 2.0 - 38.0 - 22.0 - 10.0 - 32.0).max(120.0);

        if caret_x < self.input_scroll_x + 4.0 {
            self.input_scroll_x = (caret_x - 4.0).max(0.0);
        } else if caret_x > self.input_scroll_x + available_w - 6.0 {
            self.input_scroll_x = (caret_x - (available_w - 6.0)).max(0.0);
        }
    }

    fn toggle_pin_index(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(result) = self.results.get(idx).cloned() else {
            return;
        };
        if let SearchItem::App(app) = result.item {
            let pinned = self.usage.toggle_pin(&app.id);
            let msg = if pinned {
                format!("Pinned {}", app.name)
            } else {
                format!("Unpinned {}", app.name)
            };
            self.status_hint = Some((msg, Instant::now()));
            self.recompute_results();
            cx.notify();
        }
    }

    fn on_input_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let idx = self.char_index_for_window_x(event.position.x, window);
        if event.click_count >= 3 {
            self.select_all();
            self.selecting_text = false;
        } else if event.click_count == 2 {
            self.select_word_at(idx);
            self.selecting_text = false;
        } else {
            if event.modifiers.shift {
                if self.selection_anchor.is_none() {
                    self.selection_anchor = Some(self.cursor);
                }
                self.cursor = idx;
            } else {
                self.selection_anchor = Some(idx);
                self.cursor = idx;
            }
            self.selecting_text = true;
        }
        self.reset_caret_blink(cx);
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selecting_text {
            return;
        }
        if event.pressed_button == Some(MouseButton::Left) {
            let idx = self.char_index_for_window_x(event.position.x, window);
            if self.cursor != idx {
                self.cursor = idx;
                cx.notify();
            }
        } else {
            self.selecting_text = false;
        }
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selecting_text {
            self.selecting_text = false;
            if self.selection_anchor == Some(self.cursor) {
                self.selection_anchor = None;
            }
            cx.notify();
        }
    }

    fn insert_str(&mut self, text: &str) {
        let cleaned: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if cleaned.is_empty() {
            return;
        }
        self.pending_power_confirm = None;
        self.delete_selection();
        let byte_idx = self.byte_index_at_char(self.cursor);
        self.query.insert_str(byte_idx, &cleaned);
        self.cursor += cleaned.chars().count();
        self.selection_anchor = None;
        self.selected = 0;
        self.recompute_results();
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let key = ks.key.as_str();
        let mods = ks.modifiers;

        match key {
            "escape" => {
                self.hide(cx);
                return;
            }
            "down" => {
                self.move_selection(1, cx);
                return;
            }
            "up" => {
                self.move_selection(-1, cx);
                return;
            }
            "tab" => {
                self.move_selection(if mods.shift { -1 } else { 1 }, cx);
                return;
            }
            "pagedown" => {
                self.move_selection(5, cx);
                return;
            }
            "pageup" => {
                self.move_selection(-5, cx);
                return;
            }
            "enter" => {
                self.activate_index(self.selected, mods.shift, cx);
                return;
            }
            "left" => {
                if mods.shift {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                    self.cursor = if mods.control {
                        self.prev_word_boundary(self.cursor)
                    } else {
                        self.cursor.saturating_sub(1)
                    };
                } else if let Some((start, _)) = self.selection_range() {
                    self.cursor = start;
                    self.clear_selection();
                } else if mods.control {
                    self.cursor = self.prev_word_boundary(self.cursor);
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                }
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            "right" => {
                if mods.shift {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                    self.cursor = if mods.control {
                        self.next_word_boundary(self.cursor)
                    } else {
                        (self.cursor + 1).min(self.char_len())
                    };
                } else if let Some((_, end)) = self.selection_range() {
                    self.cursor = end;
                    self.clear_selection();
                } else if mods.control {
                    self.cursor = self.next_word_boundary(self.cursor);
                } else if self.cursor < self.char_len() {
                    self.cursor += 1;
                }
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            "home" => {
                if mods.shift {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                } else {
                    self.clear_selection();
                }
                self.cursor = 0;
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            "end" => {
                if mods.shift {
                    if self.selection_anchor.is_none() {
                        self.selection_anchor = Some(self.cursor);
                    }
                } else {
                    self.clear_selection();
                }
                self.cursor = self.char_len();
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            "backspace" => {
                if self.delete_selection() {
                    self.reset_caret_blink(cx);
                    cx.notify();
                    return;
                }
                if mods.control {
                    if self.cursor > 0 {
                        let end_b = self.byte_index_at_char(self.cursor);
                        let before = &self.query[..end_b];
                        let trimmed_end = before.trim_end().len();
                        let word_start = before[..trimmed_end]
                            .rfind(char::is_whitespace)
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        self.query.replace_range(word_start..end_b, "");
                        self.cursor = self.query[..word_start].chars().count();
                        self.selected = 0;
                        self.recompute_results();
                        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                        self.reset_caret_blink(cx);
                        cx.notify();
                    }
                } else if self.cursor > 0 {
                    let start_b = self.byte_index_at_char(self.cursor - 1);
                    let end_b = self.byte_index_at_char(self.cursor);
                    self.query.replace_range(start_b..end_b, "");
                    self.cursor -= 1;
                    self.selected = 0;
                    self.recompute_results();
                    self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                    self.reset_caret_blink(cx);
                    cx.notify();
                }
                return;
            }
            "delete" => {
                if self.delete_selection() {
                    self.reset_caret_blink(cx);
                    cx.notify();
                    return;
                }
                let len = self.char_len();
                if self.cursor < len {
                    let start_b = self.byte_index_at_char(self.cursor);
                    let end_b = self.byte_index_at_char(self.cursor + 1);
                    self.query.replace_range(start_b..end_b, "");
                    self.selected = 0;
                    self.recompute_results();
                    self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                    self.reset_caret_blink(cx);
                    cx.notify();
                }
                return;
            }
            "c" if mods.control => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.status_hint = Some(("Copied".to_string(), Instant::now()));
                    cx.notify();
                }
                return;
            }
            "x" if mods.control => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.delete_selection();
                    self.reset_caret_blink(cx);
                    cx.notify();
                }
                return;
            }
            "v" if mods.control => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.insert_str(&text);
                    self.reset_caret_blink(cx);
                    cx.notify();
                }
                return;
            }
            "p" if mods.control => {
                self.toggle_pin_index(self.selected, cx);
                return;
            }
            "u" | "l" if mods.control => {
                self.query.clear();
                self.cursor = 0;
                self.clear_selection();
                self.selected = 0;
                self.recompute_results();
                self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            "a" if mods.control => {
                self.select_all();
                self.reset_caret_blink(cx);
                cx.notify();
                return;
            }
            _ => {}
        }

        if !mods.control && !mods.alt && !mods.platform {
            if let Some(ch_str) = &ks.key_char {
                if !ch_str.is_empty() && ch_str.chars().all(|c| !c.is_control()) {
                    self.insert_str(ch_str);
                    self.reset_caret_blink(cx);
                    cx.notify();
                }
            }
        }
    }
}

impl Focusable for LauncherView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LauncherView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (opacity, slide_y) = match self.anim {
            AnimState::Opening(start) => {
                let dur = self.config.window.animation_ms.max(1) as f32;
                let t = (start.elapsed().as_secs_f32() * 1000.0 / dur).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - t).powi(3);
                if t < 1.0 {
                    let entity = cx.entity_id();
                    window.on_next_frame(move |_, cx| cx.notify(entity));
                } else {
                    self.anim = AnimState::Idle;
                }
                (0.15 + 0.85 * eased, (1.0 - eased) * 12.0)
            }
            AnimState::Closing(start) => {
                let dur = self.config.window.animation_ms.clamp(60, 110) as f32;
                let t = (start.elapsed().as_secs_f32() * 1000.0 / dur).clamp(0.0, 1.0);
                let eased = 1.0 - t * t;
                if t < 1.0 {
                    let entity = cx.entity_id();
                    window.on_next_frame(move |_, cx| cx.notify(entity));
                    (eased, (1.0 - eased) * 8.0)
                } else {
                    self.is_visible = false;
                    self.anim = AnimState::Idle;
                    win_hooks::set_launcher_visible(false);
                    if let Some(hwnd) = self.hwnd {
                        win_window::hide_hwnd(hwnd);
                    }
                    (0.0, 8.0)
                }
            }
            AnimState::Idle => (1.0, 0.0),
        };

        self.sync_input_scroll(window);

        let theme = self.theme.clone();
        let radius = px(self.config.window.border_radius);
        let row_height = px(self.config.window.row_height);
        let font_family: SharedString = theme.font_family.clone().into();
        let mono_font: SharedString = theme.mono_font_family.clone().into();
        let input_scroll_x = self.input_scroll_x;
        let pending_confirm = self
            .pending_power_confirm
            .as_ref()
            .and_then(|(k, at)| (at.elapsed() < Duration::from_secs(4)).then(|| k.clone()));

        let cursor_byte = self.byte_index_at_char(self.cursor);
        let selection_slices = self.selection_range().map(|(s, e)| {
            let sb = self.byte_index_at_char(s);
            let eb = self.byte_index_at_char(e);
            (
                self.query[..sb].to_string(),
                self.query[sb..eb].to_string(),
                self.query[eb..].to_string(),
            )
        });
        let before_cursor = self.query[..cursor_byte].to_string();
        let after_cursor = self.query[cursor_byte..].to_string();
        let has_query = !self.query.is_empty();

        let results = self.results.clone();
        let selected_idx = self.selected;
        let result_count = results.len();
        let view_weak = cx.entity().downgrade();

        let status_label: SharedString = if self.indexing {
            "Indexing…".into()
        } else if let Some((msg, at)) = &self.status_hint {
            if at.elapsed() < Duration::from_secs(3) {
                msg.clone().into()
            } else {
                format!("{} apps", self.apps.len()).into()
            }
        } else {
            format!("{} apps", self.apps.len()).into()
        };

        let taskbar_hidden = taskbar::is_hidden();
        let keycap_bg: Hsla = Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.05,
        }
        .into();
        let keycap_border: Hsla = Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.09,
        }
        .into();

        // Scroll cue metrics for the ultra-thin scrollbar thumb and bottom fade
        let list_viewport_h = (self.config.window.max_height - 50.0 - 26.0 - 12.0).max(80.0);
        let total_content_h = result_count as f32 * self.config.window.row_height;
        let (scroll_offset_y, max_scroll_y) = {
            let state = self.scroll_handle.0.borrow();
            let off = (-f32::from(state.base_handle.offset().y)).max(0.0);
            let max_off = f32::from(state.base_handle.max_offset().height).max(0.0);
            (off, max_off)
        };
        let effective_max_scroll = if max_scroll_y > 0.5 {
            max_scroll_y
        } else {
            (total_content_h - list_viewport_h).max(0.0)
        };
        let is_scrollable = effective_max_scroll > 1.0;
        let track_h = list_viewport_h;
        let thumb_h = if total_content_h > 0.0 {
            ((list_viewport_h / total_content_h) * track_h).clamp(22.0, track_h)
        } else {
            track_h
        };
        let scroll_ratio = if effective_max_scroll > 0.0 {
            (scroll_offset_y / effective_max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let thumb_top = 6.0 + scroll_ratio * (track_h - thumb_h).max(0.0);
        let show_bottom_fade = is_scrollable && scroll_offset_y < effective_max_scroll - 2.0;

        div()
            .size_full()
            .pt(px(win_window::WINDOW_PAD_TOP + slide_y))
            .pb(px(win_window::WINDOW_PAD_BOTTOM - slide_y))
            .px(px(win_window::WINDOW_PAD_X))
            .font_family(font_family)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(
                div()
                    .size_full()
                    .opacity(opacity)
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(radius)
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(vec![BoxShadow {
                        color: Hsla {
                            h: 0.72,
                            s: 0.35,
                            l: 0.02,
                            a: 0.42,
                        },
                        offset: gpui::point(px(0.0), px(6.0)),
                        blur_radius: px(16.0),
                        spread_radius: px(0.0),
                    }])
                    // Flush command input header aligned to the row icon/text vertical grid
                    .child(
                        div()
                            .id("search-input-bar")
                            .h(px(50.0))
                            .w_full()
                            .px(px(19.0))
                            .bg(theme.surface_focused)
                            .border_b_1()
                            .border_color(theme.border_subtle)
                            .cursor_text()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_input_mouse_down))
                            .child(
                                div()
                                    .w(px(22.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_family(mono_font.clone())
                                    .text_size(px(13.0))
                                    .text_color(if has_query {
                                        theme.accent_secondary
                                    } else {
                                        theme.text_muted
                                    })
                                    .child("\u{f002}"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .text_size(px(theme.font_size + 1.5))
                                    .font_weight(FontWeight::NORMAL)
                                    .child(if let Some((before, selected, after)) = selection_slices {
                                        div()
                                            .flex()
                                            .items_center()
                                            .whitespace_nowrap()
                                            .flex_shrink_0()
                                            .ml(px(-input_scroll_x))
                                            .text_color(theme.text_primary)
                                            .child(before)
                                            .child(
                                                div()
                                                    .px(px(1.5))
                                                    .rounded(px(3.0))
                                                    .bg(theme.accent.opacity(0.40))
                                                    .text_color(theme.text_primary)
                                                    .child(selected),
                                            )
                                            .child(after)
                                    } else if has_query {
                                        div()
                                            .flex()
                                            .items_center()
                                            .whitespace_nowrap()
                                            .flex_shrink_0()
                                            .ml(px(-input_scroll_x))
                                            .text_color(theme.text_primary)
                                            .child(before_cursor)
                                            .child(
                                                div()
                                                    .w(px(1.5))
                                                    .h(px(17.0))
                                                    .bg(if self.caret_visible {
                                                        theme.accent
                                                    } else {
                                                        Hsla::transparent_black()
                                                    }),
                                            )
                                            .child(after_cursor)
                                    } else {
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .w(px(1.5))
                                                    .h(px(17.0))
                                                    .mr(px(2.0))
                                                    .bg(if self.caret_visible {
                                                        theme.accent
                                                    } else {
                                                        Hsla::transparent_black()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme.text_muted)
                                                    .child(self.config.search.placeholder.clone()),
                                            )
                                    }),
                            )
                            .when(has_query, |el| {
                                el.child(
                                    div()
                                        .text_size(px(11.0))
                                        .font_family(mono_font.clone())
                                        .text_color(theme.text_muted)
                                        .child(format!("{result_count}")),
                                )
                            }),
                    )
                    // Results list with subtle scrollbar thumb and bottom fade cue
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .pl(px(7.0))
                            .pr(px(10.0))
                            .py(px(6.0))
                            .child(if result_count == 0 {
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.5))
                                    .text_color(theme.text_muted)
                                    .child(if self.indexing {
                                        "Scanning installed applications…"
                                    } else {
                                        "No results"
                                    })
                                    .into_any_element()
                            } else {
                                let theme_list = theme.clone();
                                let mono_list = mono_font.clone();
                                let view_list = view_weak.clone();

                                uniform_list(
                                    "launcher-results",
                                    result_count,
                                    move |range, _window, _cx| {
                                        range
                                            .map(|ix| {
                                                let item = &results[ix];
                                                let is_sel = ix == selected_idx;
                                                render_result_row(
                                                    ix,
                                                    item,
                                                    is_sel,
                                                    pending_confirm.clone(),
                                                    row_height,
                                                    &theme_list,
                                                    &mono_list,
                                                    keycap_bg,
                                                    keycap_border,
                                                    view_list.clone(),
                                                )
                                            })
                                            .collect()
                                    },
                                )
                                .track_scroll(self.scroll_handle.clone())
                                .size_full()
                                .into_any_element()
                            })
                            .when(show_bottom_fade, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .h(px(18.0))
                                        .bg(linear_gradient(
                                            180.,
                                            linear_color_stop(theme.background.opacity(0.0), 0.),
                                            linear_color_stop(theme.background, 1.),
                                        )),
                                )
                            })
                            .when(is_scrollable, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .right(px(3.5))
                                        .top(px(thumb_top))
                                        .w(px(3.0))
                                        .h(px(thumb_h))
                                        .rounded(px(2.0))
                                        .bg(theme.text_muted.opacity(0.32)),
                                )
                            }),
                    )
                    // Ultra-minimal footer aligned to the left vertical grid
                    .child(
                        div()
                            .h(px(26.0))
                            .px(px(19.0))
                            .bg(theme.surface)
                            .border_t_1()
                            .border_color(theme.border_subtle)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .text_size(px(10.5))
                                    .text_color(theme.text_muted)
                                    .child(footer_hint("↑↓", "Move", keycap_bg, keycap_border, &mono_font, &theme))
                                    .child(footer_hint("↵", "Open", keycap_bg, keycap_border, &mono_font, &theme))
                                    .child(footer_hint("Ctrl+P", "Pin", keycap_bg, keycap_border, &mono_font, &theme))
                                    .child(footer_hint("Esc", "Close", keycap_bg, keycap_border, &mono_font, &theme)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when(taskbar_hidden, |el| {
                                        el.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .text_size(px(10.5))
                                                .text_color(theme.badge_math_text)
                                                .child(
                                                    div()
                                                        .size(px(5.0))
                                                        .rounded(px(3.0))
                                                        .bg(theme.badge_math_text),
                                                )
                                                .child("Taskbar hidden"),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_muted)
                                            .child(status_label),
                                    ),
                            ),
                    ),
            )
    }
}

fn footer_hint(
    key: &str,
    label: &str,
    keycap_bg: Hsla,
    keycap_border: Hsla,
    mono_font: &SharedString,
    theme: &ResolvedTheme,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .px(px(4.5))
                .py(px(0.5))
                .rounded(px(4.0))
                .bg(keycap_bg)
                .border_1()
                .border_color(keycap_border)
                .font_family(mono_font.clone())
                .text_size(px(9.5))
                .text_color(theme.text_secondary)
                .child(key.to_string()),
        )
        .child(div().text_color(theme.text_muted).child(label.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn render_result_row(
    ix: usize,
    result: &SearchResult,
    is_selected: bool,
    pending_confirm: Option<SystemActionKind>,
    row_height: gpui::Pixels,
    theme: &ResolvedTheme,
    mono_font: &SharedString,
    keycap_bg: Hsla,
    keycap_border: Hsla,
    view: gpui::WeakEntity<LauncherView>,
) -> impl IntoElement {
    let row_bg = if is_selected {
        theme.row_selected
    } else {
        Hsla::transparent_black()
    };
    let row_border = if is_selected {
        theme.accent.opacity(0.08)
    } else {
        Hsla::transparent_black()
    };

    let view_click = view.clone();
    let view_pin_btn = view.clone();
    let view_hover = view;
    let is_app = matches!(&result.item, SearchItem::App(_));
    let is_pinned = result.pinned;

    // Dedicated crisp Math calculator strip
    if let SearchItem::Math(m) = &result.item {
        return div()
            .h(row_height)
            .w_full()
            .py(px(1.5))
            .child(
                div()
                    .id(("row_pill", ix))
                    .size_full()
                    .rounded(px(8.0))
                    .px(px(11.0))
                    .bg(if is_selected {
                        theme.row_selected
                    } else {
                        theme.surface
                    })
                    .border_1()
                    .border_color(if is_selected {
                        theme.badge_math_text.opacity(0.20)
                    } else {
                        theme.border_subtle
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                        let _ = view_click.update(cx, |this, cx| {
                            this.activate_index(ix, false, cx);
                        });
                    })
                    .on_hover(move |hovered, _window, cx| {
                        if *hovered {
                            let _ = view_hover.update(cx, |this, cx| {
                                if this.selected != ix {
                                    this.selected = ix;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .min_w_0()
                            .child(
                                div()
                                    .w(px(22.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_family(mono_font.clone())
                                    .text_size(px(13.0))
                                    .text_color(theme.badge_math_text)
                                    .child("="),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font.clone())
                                    .text_size(px(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(m.expression.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .font_family(mono_font.clone())
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .child(m.formatted.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.badge_math_text)
                                    .child("Copy"),
                            )
                            .child(
                                div()
                                    .w(px(22.0))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .when(is_selected, |el| {
                                        el.child(return_keycap(
                                            keycap_bg,
                                            keycap_border,
                                            mono_font,
                                            theme,
                                        ))
                                    }),
                            ),
                    ),
            )
            .into_any_element();
    }

    let is_confirming =
        matches!(&result.item, SearchItem::System(s) if pending_confirm.as_ref() == Some(&s.kind));

    let (title, hint, kind_label): (String, Option<String>, &'static str) = match &result.item {
        SearchItem::Command(c) => (
            c.command.clone(),
            Some(if is_selected {
                "Shift+↵ Admin".to_string()
            } else {
                "Run command".to_string()
            }),
            "Shell",
        ),
        SearchItem::App(a) => {
            let hint = (!a.subtitle.is_empty()).then(|| a.subtitle.clone());
            let kind = match a.category {
                AppCategory::App => "App",
                AppCategory::Store => "Store",
            };
            (a.name.clone(), hint, kind)
        }
        SearchItem::System(s) => (
            s.name.clone(),
            is_confirming.then(|| "Press ↵ again".to_string()),
            if is_confirming {
                "Confirm?"
            } else {
                match s.kind {
                    SystemActionKind::SetTheme(_) => "Theme",
                    _ if s.is_power => "Power",
                    _ => "System",
                }
            },
        ),
        SearchItem::Math(_) => unreachable!(),
    };

    let icon_el = match &result.item {
        SearchItem::Command(_) => div()
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .font_family(mono_font.clone())
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.badge_store_text)
            .child(">_")
            .into_any_element(),
        SearchItem::App(app) => {
            if let Some(render_img) = &app.icon {
                div()
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .child(img(render_img.clone()).size(px(21.0)))
                    .into_any_element()
            } else {
                let initial = app
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string())
                    .unwrap_or_else(|| "A".to_string());
                let tint = match app.category {
                    AppCategory::Store => theme.badge_store_text,
                    AppCategory::App => theme.accent_secondary,
                };
                div()
                    .size(px(21.0))
                    .rounded(px(5.0))
                    .bg(theme.row_hover)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tint)
                    .child(initial)
                    .into_any_element()
            }
        }
        SearchItem::System(sys) => {
            if let SystemActionKind::SetTheme(choice) = &sys.kind {
                let (bg_hex, accent_hex) = choice.preview_colors();
                let swatch_bg = config::parse_hex_color(bg_hex, "#1a1b26ff");
                let swatch_accent = config::parse_hex_color(accent_hex, "#7aa2f7ff");
                div()
                    .size(px(20.0))
                    .rounded(px(6.0))
                    .bg(swatch_bg)
                    .border_1()
                    .border_color(swatch_accent.opacity(0.45))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded(px(4.0))
                            .bg(swatch_accent),
                    )
                    .into_any_element()
            } else {
                let (glyph, tint) = match sys.kind {
                    SystemActionKind::Lock => ("\u{f023}", theme.accent_secondary),
                    SystemActionKind::Sleep => ("\u{23fe}", theme.badge_store_text),
                    SystemActionKind::Restart => ("\u{ead2}", theme.badge_system_text),
                    SystemActionKind::Shutdown => ("\u{f011}", theme.badge_system_text),
                    SystemActionKind::ReloadApps => ("\u{f021}", theme.badge_math_text),
                    SystemActionKind::ToggleTaskbar => ("\u{f2d0}", theme.accent_secondary),
                    SystemActionKind::ToggleWinKeyHook => ("\u{f17a}", theme.accent_secondary),
                    SystemActionKind::OpenConfig => ("\u{f013}", theme.text_secondary),
                    SystemActionKind::CheckUpdates => ("\u{f0aa}", theme.badge_store_text),
                    SystemActionKind::InstallUpdate => ("\u{f019}", theme.badge_math_text),
                    SystemActionKind::SetTheme(_) => unreachable!(),
                };
                div()
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(mono_font.clone())
                    .text_size(px(13.5))
                    .text_color(if is_selected {
                        tint
                    } else {
                        tint.opacity(0.82)
                    })
                    .child(glyph)
                    .into_any_element()
            }
        }
        SearchItem::Math(_) => unreachable!(),
    };

    let title_color = if is_selected {
        theme.text_primary
    } else {
        theme.text_primary.opacity(0.90)
    };
    let kind_color = if is_confirming {
        theme.badge_system_text
    } else if is_selected {
        theme.text_secondary
    } else {
        theme.text_muted
    };

    div()
        .h(row_height)
        .w_full()
        .py(px(1.5))
        .child(
            div()
                .id(("row_pill", ix))
                .size_full()
                .rounded(px(8.0))
                .px(px(11.0))
                .bg(row_bg)
                .border_1()
                .border_color(row_border)
                .hover(|s| {
                    if is_selected {
                        s
                    } else {
                        s.bg(theme.row_hover)
                    }
                })
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(10.0))
                .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                    let _ = view_click.update(cx, |this, cx| {
                        this.activate_index(ix, false, cx);
                    });
                })
                .on_hover(move |hovered, _window, cx| {
                    if *hovered {
                        let _ = view_hover.update(cx, |this, cx| {
                            if this.selected != ix {
                                this.selected = ix;
                                cx.notify();
                            }
                        });
                    }
                })
                .child(icon_el)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_highlighted_title(
                            &title,
                            &result.matched_indices,
                            title_color,
                            theme.accent,
                            px(theme.font_size),
                            is_selected,
                        ))
                        .when_some(hint, |el, h| {
                            el.child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(if is_confirming {
                                        theme.badge_system_text
                                    } else {
                                        theme.text_muted
                                    })
                                    .child(h),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(if is_app && (is_pinned || is_selected) {
                            let pin_color = if is_pinned {
                                theme.accent
                            } else {
                                theme.text_muted.opacity(0.55)
                            };
                            div()
                                .id(("row_pin", ix))
                                .w(px(18.0))
                                .h(px(20.0))
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .font_family(mono_font.clone())
                                .text_size(px(10.5))
                                .text_color(pin_color)
                                .hover(|s| s.text_color(theme.accent))
                                .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                                    cx.stop_propagation();
                                    let _ = view_pin_btn.update(cx, |this, cx| {
                                        this.toggle_pin_index(ix, cx);
                                    });
                                })
                                .child("\u{f08d}")
                                .into_any_element()
                        } else {
                            div().w(px(18.0)).into_any_element()
                        })
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(kind_color)
                                .child(kind_label),
                        )
                        .child(
                            div()
                                .w(px(22.0))
                                .flex()
                                .items_center()
                                .justify_end()
                                .when(is_selected, |el| {
                                    el.child(return_keycap(
                                        keycap_bg,
                                        keycap_border,
                                        mono_font,
                                        theme,
                                    ))
                                }),
                        ),
                ),
        )
        .into_any_element()
}

fn render_highlighted_title(
    title: &str,
    matched_indices: &[u32],
    base_color: Hsla,
    accent_color: Hsla,
    font_size: Pixels,
    is_selected: bool,
) -> gpui::AnyElement {
    let base_weight = if is_selected {
        FontWeight::MEDIUM
    } else {
        FontWeight::NORMAL
    };

    if matched_indices.is_empty() {
        return div()
            .truncate()
            .text_size(font_size)
            .font_weight(base_weight)
            .text_color(base_color)
            .child(title.to_string())
            .into_any_element();
    }

    let mut container = div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .text_size(font_size);

    let mut cursor = 0usize;
    let mut current_matched: Option<bool> = None;
    let mut run_start_byte = 0usize;

    for (char_idx, (byte_idx, _ch)) in title.char_indices().enumerate() {
        while cursor < matched_indices.len() && (matched_indices[cursor] as usize) < char_idx {
            cursor += 1;
        }
        let is_match = matched_indices
            .get(cursor)
            .is_some_and(|&m| m as usize == char_idx);

        match current_matched {
            Some(prev) if prev != is_match => {
                let slice = &title[run_start_byte..byte_idx];
                container = container.child(
                    div()
                        .whitespace_nowrap()
                        .flex_shrink_0()
                        .font_weight(if prev { FontWeight::SEMIBOLD } else { base_weight })
                        .text_color(if prev { accent_color } else { base_color })
                        .child(slice.to_string()),
                );
                run_start_byte = byte_idx;
                current_matched = Some(is_match);
            }
            None => current_matched = Some(is_match),
            _ => {}
        }
    }

    if let Some(prev) = current_matched {
        if run_start_byte < title.len() {
            let slice = &title[run_start_byte..];
            container = container.child(
                div()
                    .whitespace_nowrap()
                    .flex_shrink_0()
                    .font_weight(if prev { FontWeight::SEMIBOLD } else { base_weight })
                    .text_color(if prev { accent_color } else { base_color })
                    .child(slice.to_string()),
            );
        }
    }

    container.into_any_element()
}

fn return_keycap(
    keycap_bg: Hsla,
    keycap_border: Hsla,
    mono_font: &SharedString,
    theme: &ResolvedTheme,
) -> impl IntoElement {
    div()
        .px(px(5.0))
        .py(px(0.5))
        .rounded(px(4.0))
        .bg(keycap_bg)
        .border_1()
        .border_color(keycap_border)
        .font_family(mono_font.clone())
        .text_size(px(10.0))
        .text_color(theme.text_secondary)
        .child("↵")
}
