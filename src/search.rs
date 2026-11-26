use std::sync::Arc;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as NucleoConfig, Matcher, Utf32Str};

use crate::config::SearchConfig;
use crate::indexer::{builtin_system_actions, IndexedApp, SystemAction};
use crate::math::{self, MathResult};
use crate::usage::UsageStore;

#[derive(Debug, Clone)]
pub struct RunCommand {
    pub command: String,
}

#[derive(Debug, Clone)]
pub enum SearchItem {
    Math(MathResult),
    Command(RunCommand),
    App(Arc<IndexedApp>),
    System(SystemAction),
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub item: SearchItem,
    pub score: i64,
    pub pinned: bool,
    pub matched_indices: Vec<u32>,
}

impl SearchResult {
    pub fn title(&self) -> &str {
        match &self.item {
            SearchItem::Math(m) => &m.formatted,
            SearchItem::Command(c) => &c.command,
            SearchItem::App(a) => &a.name,
            SearchItem::System(s) => &s.name,
        }
    }
}

pub struct SearchEngine {
    matcher: Matcher,
}

impl SearchEngine {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(NucleoConfig::DEFAULT),
        }
    }

    pub fn search(
        &mut self,
        query: &str,
        apps: &[Arc<IndexedApp>],
        usage: &UsageStore,
        cfg: &SearchConfig,
        taskbar_hidden: bool,
        intercept_win_key: bool,
    ) -> Vec<SearchResult> {
        let trimmed = query.trim();

        if trimmed.starts_with('>') {
            let cmd_str = trimmed.trim_start_matches('>').trim();
            if cmd_str.is_empty() {
                return Vec::new();
            }
            return vec![SearchResult {
                item: SearchItem::Command(RunCommand {
                    command: cmd_str.to_string(),
                }),
                score: i64::MAX,
                pinned: false,
                matched_indices: Vec::new(),
            }];
        }

        let system_actions = if cfg.show_system_actions {
            builtin_system_actions(taskbar_hidden, intercept_win_key)
        } else {
            Vec::new()
        };

        if trimmed.is_empty() {
            let mut results: Vec<SearchResult> = apps
                .iter()
                .map(|app| {
                    let entry = usage.get(&app.id);
                    let count = entry.map(|e| e.count as i64).unwrap_or(0);
                    let recency = entry.map(|e| e.last_used_unix as i64).unwrap_or(0);
                    let pinned = usage.is_pinned(&app.id);
                    let score = count * 1_000_000 + (recency % 1_000_000);
                    SearchResult {
                        item: SearchItem::App(app.clone()),
                        score,
                        pinned,
                        matched_indices: Vec::new(),
                    }
                })
                .collect();

            results.sort_by(|a, b| {
                b.pinned
                    .cmp(&a.pinned)
                    .then_with(|| {
                        if a.pinned && b.pinned {
                            let pa = match &a.item {
                                SearchItem::App(app) => usage.pinned_at(&app.id),
                                _ => 0,
                            };
                            let pb = match &b.item {
                                SearchItem::App(app) => usage.pinned_at(&app.id),
                                _ => 0,
                            };
                            pa.cmp(&pb)
                        } else {
                            b.score.cmp(&a.score)
                        }
                    })
                    .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
            });

            results.truncate(cfg.max_results.saturating_sub(system_actions.len().min(6)));
            for sys in system_actions {
                results.push(SearchResult {
                    item: SearchItem::System(sys),
                    score: -1,
                    pinned: false,
                    matched_indices: Vec::new(),
                });
            }
            results.truncate(cfg.max_results);
            return results;
        }

        let mut results = Vec::new();

        if cfg.show_math {
            if let Some(m) = math::try_eval(trimmed) {
                results.push(SearchResult {
                    item: SearchItem::Math(m),
                    score: i64::MAX,
                    pinned: false,
                    matched_indices: Vec::new(),
                });
            }
        }

        let q_lower = trimmed.to_lowercase();
        let pattern = Pattern::parse(trimmed, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let mut ranked: Vec<SearchResult> = Vec::new();

        for app in apps {
            if let Some(base) = score_candidate(
                &mut self.matcher,
                &pattern,
                &q_lower,
                &app.name,
                &app.keywords,
                &mut buf,
            ) {
                let pinned = usage.is_pinned(&app.id);
                let total = base + usage.boost(&app.id) + if pinned { 180 } else { 0 };
                ranked.push(SearchResult {
                    item: SearchItem::App(app.clone()),
                    score: total,
                    pinned,
                    matched_indices: Vec::new(),
                });
            }
        }

        for sys in system_actions {
            if let Some(base) = score_candidate(
                &mut self.matcher,
                &pattern,
                &q_lower,
                &sys.name,
                &sys.keywords,
                &mut buf,
            ) {
                ranked.push(SearchResult {
                    item: SearchItem::System(sys),
                    score: base,
                    pinned: false,
                    matched_indices: Vec::new(),
                });
            }
        }

        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
        });

        let remaining = cfg.max_results.saturating_sub(results.len());
        ranked.truncate(remaining);

        for res in &mut ranked {
            let mut idxs = Vec::new();
            let hay = Utf32Str::new(res.title(), &mut buf);
            pattern.indices(hay, &mut self.matcher, &mut idxs);
            idxs.sort_unstable();
            idxs.dedup();
            res.matched_indices = idxs;
        }

        results.extend(ranked);
        results
    }
}

fn score_candidate(
    matcher: &mut Matcher,
    pattern: &Pattern,
    q_lower: &str,
    name: &str,
    keywords: &str,
    buf: &mut Vec<char>,
) -> Option<i64> {
    let name_hay = Utf32Str::new(name, buf);
    let primary_score = pattern.score(name_hay, matcher).map(|s| s as i64);

    let kw_score = if !keywords.is_empty() {
        let kw_hay = Utf32Str::new(keywords, buf);
        pattern.score(kw_hay, matcher).map(|s| (s as i64 * 6) / 10)
    } else {
        None
    };

    let base = match (primary_score, kw_score) {
        (Some(a), Some(b)) => a.max(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };

    let name_lower = name.to_lowercase();
    let mut bonus = 0i64;
    if name_lower == q_lower {
        bonus += 600;
    } else if name_lower.starts_with(q_lower) {
        bonus += 300;
    } else if name_lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(q_lower))
    {
        bonus += 120;
    }

    Some(base + bonus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{AppCategory, LaunchTarget};
    use std::path::PathBuf;

    fn dummy_app(id: &str, name: &str, keywords: &str) -> Arc<IndexedApp> {
        Arc::new(IndexedApp {
            id: id.to_string(),
            name: name.to_string(),
            subtitle: "Desktop App".to_string(),
            category: AppCategory::App,
            launch: LaunchTarget::Path(PathBuf::from("C:\\dummy.exe")),
            keywords: keywords.to_string(),
            icon: None,
        })
    }

    #[test]
    fn ranks_prefix_and_frequency_higher() {
        let apps = vec![
            dummy_app("1", "My Code Helper", "editor"),
            dummy_app("2", "Visual Studio Code", "vscode editor"),
            dummy_app("3", "CodeWriter", "notes"),
        ];
        let mut usage = UsageStore::default();
        usage.record_launch("2");
        usage.record_launch("2");
        usage.record_launch("2");

        let mut engine = SearchEngine::new();
        let cfg = SearchConfig::default();

        let res = engine.search("code", &apps, &usage, &cfg, false, false);
        assert!(!res.is_empty());
        assert_eq!(res[0].title(), "CodeWriter");

        let res_vs = engine.search("vscode", &apps, &usage, &cfg, false, false);
        assert_eq!(res_vs[0].title(), "Visual Studio Code");
    }

    #[test]
    fn puts_math_first_when_valid() {
        let apps = vec![dummy_app("1", "Calculator", "math")];
        let usage = UsageStore::default();
        let mut engine = SearchEngine::new();
        let cfg = SearchConfig::default();

        let res = engine.search("24*12", &apps, &usage, &cfg, false, false);
        assert!(!res.is_empty());
        assert!(matches!(res[0].item, SearchItem::Math(_)));
        assert_eq!(res[0].title(), "288");
    }

    #[test]
    fn pins_sort_above_unpinned_on_empty_query() {
        let apps = vec![
            dummy_app("heavy", "Heavy Used App", "app"),
            dummy_app("pin1", "Pinned Later Alph", "app"),
            dummy_app("pin2", "Zebra Pinned", "app"),
        ];
        let mut usage = UsageStore::default();
        for _ in 0..10 {
            usage.record_launch("heavy");
        }
        usage.toggle_pin("pin2");
        usage.toggle_pin("pin1");

        let mut engine = SearchEngine::new();
        let cfg = SearchConfig {
            show_system_actions: false,
            ..SearchConfig::default()
        };

        let res = engine.search("", &apps, &usage, &cfg, false, false);
        assert_eq!(res.len(), 3);
        assert!(res[0].pinned);
        assert!(res[1].pinned);
        assert!(!res[2].pinned);
        assert_eq!(res[2].title(), "Heavy Used App");
    }

    #[test]
    fn command_prefix_creates_shell_item_and_populates_indices() {
        let apps = vec![dummy_app("vsc", "Visual Studio Code", "editor")];
        let usage = UsageStore::default();
        let mut engine = SearchEngine::new();
        let cfg = SearchConfig::default();

        let cmd_res = engine.search("> ipconfig /all", &apps, &usage, &cfg, false, false);
        assert_eq!(cmd_res.len(), 1);
        assert!(matches!(cmd_res[0].item, SearchItem::Command(_)));
        assert_eq!(cmd_res[0].title(), "ipconfig /all");

        let fuzzy = engine.search("vsc", &apps, &usage, &cfg, false, false);
        assert!(!fuzzy.is_empty());
        assert!(!fuzzy[0].matched_indices.is_empty());
    }
}
