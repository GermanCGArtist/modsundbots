use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::{MubError, Result};

/// Centralized directory layout for ModsUndBots.
///
/// Assets, libraries, and version jars are shared across all instances to
/// minimize disk usage when running dozens of player and bot profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn default_root() -> Result<Self> {
        let proj = ProjectDirs::from("dev", "modsundbots", "ModsUndBots")
            .ok_or_else(|| MubError::Launch("Unable to determine OS app data directory".into()))?;
        Ok(Self {
            root: proj.data_local_dir().to_path_buf(),
        })
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn shared_dir(&self) -> PathBuf {
        self.root.join("shared")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.shared_dir().join("assets")
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.shared_dir().join("libraries")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.shared_dir().join("versions")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn instance_dir(&self, profile_id: &str) -> PathBuf {
        self.instances_dir().join(profile_id)
    }

    pub fn natives_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join(".natives")
    }

    pub fn mods_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("mods")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.root,
            &self.shared_dir(),
            &self.assets_dir(),
            &self.libraries_dir(),
            &self.versions_dir(),
            &self.instances_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
