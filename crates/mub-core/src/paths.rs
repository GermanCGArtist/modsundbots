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
    /// The app data folder; `MUB_HOME` overrides it (separate test setups, portable installs).
    pub fn default_root() -> Result<Self> {
        if let Some(home) = std::env::var_os("MUB_HOME").filter(|h| !h.is_empty()) {
            return Ok(Self {
                root: PathBuf::from(home),
            });
        }
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

    /// Instances started by any mub process (GUI, CLI, MCP) that may still be running.
    pub fn running_file(&self) -> PathBuf {
        self.root.join("running.json")
    }

    pub fn collections_file(&self) -> PathBuf {
        self.root.join("collections.json")
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

    pub fn resourcepacks_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("resourcepacks")
    }

    pub fn shaderpacks_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("shaderpacks")
    }

    pub fn datapacks_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("datapacks")
    }

    pub fn saves_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("saves")
    }

    pub fn screenshots_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("screenshots")
    }

    pub fn config_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("config")
    }

    pub fn logs_dir(&self, profile_id: &str) -> PathBuf {
        self.instance_dir(profile_id).join("logs")
    }

    pub fn icon_cache_dir(&self) -> PathBuf {
        self.shared_dir().join("cache").join("icons")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.root,
            &self.shared_dir(),
            &self.assets_dir(),
            &self.libraries_dir(),
            &self.versions_dir(),
            &self.instances_dir(),
            &self.icon_cache_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Create `path` if missing (when it has no extension or doesn't exist) and open it in the OS file manager.
pub fn open_in_file_manager(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }

    Ok(())
}
