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
    Quilt { loader_version: String },
    Forge { loader_version: String },
}

impl ModLoader {
    pub fn slug(&self) -> Option<&'static str> {
        match self {
            ModLoader::Vanilla => None,
            ModLoader::Fabric { .. } => Some("fabric"),
            ModLoader::NeoForge { .. } => Some("neoforge"),
            ModLoader::Quilt { .. } => Some("quilt"),
            ModLoader::Forge { .. } => Some("forge"),
        }
    }

    pub fn curseforge_loader_id(&self) -> Option<u32> {
        match self {
            ModLoader::Vanilla => None,
            ModLoader::Forge { .. } => Some(1),
            ModLoader::Fabric { .. } => Some(4),
            ModLoader::Quilt { .. } => Some(5),
            ModLoader::NeoForge { .. } => Some(6),
        }
    }

    pub fn loader_version(&self) -> Option<&str> {
        match self {
            ModLoader::Vanilla => None,
            ModLoader::Fabric { loader_version }
            | ModLoader::NeoForge { loader_version }
            | ModLoader::Quilt { loader_version }
            | ModLoader::Forge { loader_version } => Some(loader_version.as_str()),
        }
    }
}

impl std::fmt::Display for ModLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModLoader::Vanilla => write!(f, "Vanilla"),
            ModLoader::Fabric { loader_version } => write!(f, "Fabric {loader_version}"),
            ModLoader::NeoForge { loader_version } => write!(f, "NeoForge {loader_version}"),
            ModLoader::Quilt { loader_version } => write!(f, "Quilt {loader_version}"),
            ModLoader::Forge { loader_version } => write!(f, "Forge {loader_version}"),
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
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub source_pack: Option<String>,
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
            group: None,
            playtime_secs: 0,
            source_pack: None,
            created_at: Utc::now(),
            last_launched_at: None,
        }
    }

    /// Clone this profile's configuration under a new ID and name.
    pub fn clone_with_name(&self, name: impl Into<String>) -> Self {
        let mut cloned = self.clone();
        cloned.id = Uuid::new_v4().to_string();
        cloned.name = name.into();
        cloned.created_at = Utc::now();
        cloned.last_launched_at = None;
        cloned.playtime_secs = 0;
        cloned
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

fn default_theme_palette() -> String {
    "synth_terminal".to_string()
}

pub const SUPPORTED_THEME_PALETTES: &[&str] = &[
    "synth_terminal",
    "gruvbox_pixel",
    "tokyo_night",
    "drafting_paper",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSettings {
    pub default_java_path: String,
    pub msa_client_id: String,
    /// Delay in milliseconds between launching consecutive instances in a batch.
    pub batch_launch_stagger_ms: u64,
    /// Max concurrent file downloads when preparing a version.
    pub max_concurrent_downloads: usize,
    #[serde(default = "default_theme_palette")]
    pub theme_palette: String,
    #[serde(default)]
    pub curseforge_api_key: String,
    #[serde(default)]
    pub github_update_token: String,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            default_java_path: "java".to_string(),
            msa_client_id: crate::account::DEFAULT_MSA_CLIENT_ID.to_string(),
            batch_launch_stagger_ms: 1500,
            max_concurrent_downloads: 16,
            theme_palette: default_theme_palette(),
            curseforge_api_key: String::new(),
            github_update_token: String::new(),
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

    /// Returns the CurseForge API key from settings if non-empty, else from `MUB_CURSEFORGE_API_KEY`.
    pub fn effective_curseforge_api_key(&self) -> Option<String> {
        let trimmed = self.curseforge_api_key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
        std::env::var("MUB_CURSEFORGE_API_KEY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    /// Returns the GitHub PAT for private-repo self-updates if non-empty, else from `MUB_GITHUB_TOKEN` / `GITHUB_TOKEN`.
    pub fn effective_github_update_token(&self) -> Option<String> {
        let trimmed = self.github_update_token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
        std::env::var("MUB_GITHUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }
}
