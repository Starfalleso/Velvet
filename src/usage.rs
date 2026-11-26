use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageEntry {
    pub count: u32,
    pub last_used_unix: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub pinned_at_unix: u64,
}

#[derive(Debug, Clone, Default)]
pub struct UsageStore {
    path: PathBuf,
    entries: HashMap<String, UsageEntry>,
}

impl UsageStore {
    pub fn load_default() -> Self {
        let path = default_usage_path();
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<HashMap<String, UsageEntry>>(&raw).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn record_launch(&mut self, id: &str) {
        let now = unix_now_secs();
        let entry = self.entries.entry(id.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last_used_unix = now;
        let _ = self.save();
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.entries.get(id).map(|e| e.pinned).unwrap_or(false)
    }

    pub fn pinned_at(&self, id: &str) -> u64 {
        self.entries.get(id).map(|e| e.pinned_at_unix).unwrap_or(0)
    }

    pub fn toggle_pin(&mut self, id: &str) -> bool {
        let now = unix_now_secs();
        let entry = self.entries.entry(id.to_string()).or_default();
        entry.pinned = !entry.pinned;
        entry.pinned_at_unix = if entry.pinned { now.max(1) } else { 0 };
        let pinned = entry.pinned;
        let _ = self.save();
        pinned
    }

    pub fn get(&self, id: &str) -> Option<&UsageEntry> {
        self.entries.get(id)
    }

    pub fn boost(&self, id: &str) -> i64 {
        match self.entries.get(id) {
            Some(entry) if entry.count > 0 => ((entry.count as f64).ln_1p() * 45.0) as i64,
            _ => 0,
        }
    }

    fn save(&self) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        let _ = fs::rename(&tmp, &self.path).or_else(|_| {
            fs::copy(&tmp, &self.path)?;
            fs::remove_file(&tmp)
        });
        Ok(())
    }
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn default_usage_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("velvet")
        .join("usage.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_pin_flips_state_and_preserves_count() {
        let mut store = UsageStore::default();
        store.record_launch("app.alpha");
        store.record_launch("app.alpha");
        assert!(!store.is_pinned("app.alpha"));

        assert!(store.toggle_pin("app.alpha"));
        assert!(store.is_pinned("app.alpha"));
        assert!(store.pinned_at("app.alpha") > 0);
        assert_eq!(store.get("app.alpha").unwrap().count, 2);

        assert!(!store.toggle_pin("app.alpha"));
        assert!(!store.is_pinned("app.alpha"));
        assert_eq!(store.pinned_at("app.alpha"), 0);
        assert_eq!(store.get("app.alpha").unwrap().count, 2);
    }
}
