use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

/// A named collection of bot (or player) profiles that can be prepared and
/// started together with a single "Start All" action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCollection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub profile_ids: Vec<String>,
    /// Optional per-collection stagger delay (ms) overriding `LauncherSettings::batch_launch_stagger_ms`.
    #[serde(default)]
    pub stagger_ms: Option<u64>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_started_at: Option<DateTime<Utc>>,
}

impl BotCollection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            description: String::new(),
            profile_ids: Vec::new(),
            stagger_ms: None,
            created_at: Utc::now(),
            last_started_at: None,
        }
    }

    pub fn add_profile(&mut self, profile_id: impl Into<String>) -> bool {
        let id = profile_id.into();
        if self.profile_ids.iter().any(|existing| existing == &id) {
            false
        } else {
            self.profile_ids.push(id);
            true
        }
    }

    pub fn remove_profile(&mut self, profile_id: &str) -> bool {
        let before = self.profile_ids.len();
        self.profile_ids.retain(|id| id != profile_id);
        self.profile_ids.len() != before
    }

    pub fn mark_started(&mut self) {
        self.last_started_at = Some(Utc::now());
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BotCollectionStore {
    #[serde(default)]
    pub collections: Vec<BotCollection>,
}

impl BotCollectionStore {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&BotCollection> {
        self.collections.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut BotCollection> {
        self.collections.iter_mut().find(|c| c.id == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&BotCollection> {
        self.collections
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn find_by_name_mut(&mut self, name: &str) -> Option<&mut BotCollection> {
        self.collections
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn resolve(&self, id_or_name: &str) -> Option<&BotCollection> {
        self.get(id_or_name)
            .or_else(|| self.find_by_name(id_or_name))
    }

    pub fn resolve_mut(&mut self, id_or_name: &str) -> Option<&mut BotCollection> {
        if let Some(idx) = self
            .collections
            .iter()
            .position(|c| c.id == id_or_name || c.name.eq_ignore_ascii_case(id_or_name))
        {
            Some(&mut self.collections[idx])
        } else {
            None
        }
    }

    pub fn upsert(&mut self, collection: BotCollection) {
        if let Some(existing) = self.collections.iter_mut().find(|c| c.id == collection.id) {
            *existing = collection;
        } else {
            self.collections.push(collection);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<BotCollection> {
        if let Some(idx) = self.collections.iter().position(|c| c.id == id) {
            Some(self.collections.remove(idx))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_store_roundtrip_and_dedup() {
        let dir = std::env::temp_dir().join(format!("mub_col_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("collections.json");

        let mut store = BotCollectionStore::default();
        let mut col = BotCollection::new("FarmSquad");
        col.description = "Auto-fishing bots".to_string();
        col.stagger_ms = Some(2000);
        assert!(col.add_profile("p1"));
        assert!(!col.add_profile("p1")); // dedup
        assert!(col.add_profile("p2"));
        assert!(col.remove_profile("p1"));
        assert_eq!(col.profile_ids, vec!["p2".to_string()]);

        let id = col.id.clone();
        store.upsert(col);
        store.save(&file).unwrap();

        let loaded = BotCollectionStore::load(&file).unwrap();
        assert_eq!(loaded.collections.len(), 1);
        let c = loaded.resolve("farmsquad").unwrap();
        assert_eq!(c.id, id);
        assert_eq!(c.stagger_ms, Some(2000));
        assert_eq!(c.profile_ids, vec!["p2".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
