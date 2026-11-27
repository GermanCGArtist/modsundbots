//! `running.json`: which game processes mub started, so a restarted mub-gui can find them again.
//!
//! Every launch (GUI, CLI or MCP) records the PID together with the process creation time; a record
//! only counts while that exact process is alive, so a reused PID is never mistaken for a bot.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningRecord {
    pub profile_id: String,
    pub profile_name: String,
    pub account_name: String,
    pub pid: u32,
    /// Process creation time (FILETIME ticks on Windows, 0 if unknown).
    pub start_time: u64,
    pub started_at: DateTime<Utc>,
}

impl RunningRecord {
    /// The recorded process is still the same running process.
    pub fn is_alive(&self) -> bool {
        if !crate::window::process_alive(self.pid) {
            return false;
        }
        match crate::window::process_start_time(self.pid) {
            Some(t) => self.start_time == 0 || t == self.start_time,
            None => self.start_time == 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunningStore {
    #[serde(default)]
    pub instances: Vec<RunningRecord>,
}

/// Serializes read-modify-write cycles inside one process.
static LOCK: Mutex<()> = Mutex::new(());

impl RunningStore {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Records that are still alive (dead ones are dropped from the file).
    pub fn alive(path: &Path) -> Vec<RunningRecord> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = Self::load(path);
        let before = store.instances.len();
        store.instances.retain(RunningRecord::is_alive);
        if store.instances.len() != before {
            let _ = store.save(path);
        }
        store.instances
    }

    /// Add or replace the record for this profile.
    pub fn upsert(path: &Path, rec: RunningRecord) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = Self::load(path);
        store.instances.retain(|r| r.profile_id != rec.profile_id);
        store.instances.push(rec);
        let _ = store.save(path);
    }

    /// Remove the record of this profile's process `pid`.
    pub fn remove(path: &Path, profile_id: &str, pid: u32) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = Self::load(path);
        let before = store.instances.len();
        store
            .instances
            .retain(|r| !(r.profile_id == profile_id && r.pid == pid));
        if store.instances.len() != before {
            let _ = store.save(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, pid: u32, start: u64) -> RunningRecord {
        RunningRecord {
            profile_id: id.into(),
            profile_name: id.into(),
            account_name: "bot".into(),
            pid,
            start_time: start,
            started_at: Utc::now(),
        }
    }

    #[test]
    fn round_trip_upsert_remove_and_alive() {
        let dir = std::env::temp_dir().join(format!("mub-running-{}", uuid::Uuid::new_v4()));
        let path = dir.join("running.json");
        let me = std::process::id();
        let my_start = crate::window::process_start_time(me).unwrap_or(0);

        RunningStore::upsert(&path, rec("a", me, my_start));
        RunningStore::upsert(&path, rec("b", u32::MAX - 1, 1));
        RunningStore::upsert(&path, rec("a", me, my_start)); // replaces, no duplicate
        assert_eq!(RunningStore::load(&path).instances.len(), 2);

        // "b" is dead and gets pruned; "a" (this test process) is alive.
        let alive = RunningStore::alive(&path);
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].profile_id, "a");
        assert_eq!(RunningStore::load(&path).instances.len(), 1);

        // A record with the right PID but another start time is a reused PID, not ours.
        if my_start != 0 {
            assert!(!rec("c", me, my_start + 1).is_alive());
        }

        RunningStore::remove(&path, "a", me);
        assert!(RunningStore::load(&path).instances.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
