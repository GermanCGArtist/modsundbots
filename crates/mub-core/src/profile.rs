use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModLoader {
    Vanilla,
    Fabric { loader_version: String },
    NeoForge { loader_version: String },
}

impl std::fmt::Display for ModLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModLoader::Vanilla => write!(f, "Vanilla"),
            ModLoader::Fabric { loader_version } => write!(f, "Fabric {loader_version}"),
            ModLoader::NeoForge { loader_version } => write!(f, "NeoForge {loader_version}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Player,
    Bot,
}

impl std::fmt::Display for ProfileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileKind::Player => write!(f, "Player"),
            ProfileKind::Bot => write!(f, "Bot"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    pub mc_version: String,
    pub mod_loader: ModLoader,
    /// Optional assigned account ID.
    pub account_id: Option<String>,
    /// Override Java executable path for this profile.
    pub java_path: Option<String>,
    /// Minimum JVM heap in MB (e.g. 512).
    pub memory_min_mb: u32,
    /// Maximum JVM heap in MB (e.g. 2048 for players, 768-1024 for bots).
    pub memory_max_mb: u32,
    /// Extra JVM arguments.
    #[serde(default)]
    pub jvm_args: Vec<String>,
    /// Optional server address to quick-connect on launch (host:port).
    pub quick_connect: Option<String>,
    /// Optional window resolution (width, height).
    pub resolution: Option<(u32, u32)>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_launched_at: Option<DateTime<Utc>>,
}

impl Profile {
    pub fn new(
        name: impl Into<String>,
        kind: ProfileKind,
        mc_version: impl Into<String>,
        mod_loader: ModLoader,
    ) -> Self {
        let (memory_min_mb, memory_max_mb) = match kind {
            ProfileKind::Player => (512, 4096),
            ProfileKind::Bot => (256, 1024),
        };
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            mc_version: mc_version.into(),
            mod_loader,
            account_id: None,
            java_path: None,
            memory_min_mb,
            memory_max_mb,
            jvm_args: Vec::new(),
            quick_connect: None,
            resolution: match kind {
                ProfileKind::Player => None,
                ProfileKind::Bot => Some((640, 360)),
            },
            tags: Vec::new(),
            created_at: Utc::now(),
            last_launched_at: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
}

impl ProfileStore {
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

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn upsert(&mut self, profile: Profile) {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Profile> {
        if let Some(idx) = self.profiles.iter().position(|p| p.id == id) {
            Some(self.profiles.remove(idx))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherSettings {
    pub default_java_path: String,
    pub msa_client_id: String,
    /// Delay in milliseconds between launching consecutive instances in a batch.
    pub batch_launch_stagger_ms: u64,
    /// Max concurrent file downloads when preparing a version.
    pub max_concurrent_downloads: usize,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            default_java_path: "java".to_string(),
            msa_client_id: crate::account::DEFAULT_MSA_CLIENT_ID.to_string(),
            batch_launch_stagger_ms: 1500,
            max_concurrent_downloads: 16,
        }
    }
}

impl LauncherSettings {
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
}
