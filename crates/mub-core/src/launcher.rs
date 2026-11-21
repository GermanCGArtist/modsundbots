use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;

use crate::account::Account;
use crate::error::{MubError, Result};
use crate::mojang::{PreparedVersion, VersionManager};
use crate::paths::AppPaths;
use crate::profile::{LauncherSettings, ModLoader, Profile};

const MAX_LOG_LINES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceState {
    Preparing,
    Running,
    Stopped { exit_code: Option<i32> },
    Failed { error: String },
}

#[derive(Debug, Clone)]
pub struct InstanceStatus {
    pub profile_id: String,
    pub profile_name: String,
    pub account_name: String,
    pub pid: Option<u32>,
    pub state: InstanceState,
    pub progress: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub logs: VecDeque<String>,
}

impl InstanceStatus {
    pub fn elapsed_secs(&self) -> i64 {
        let end = self.ended_at.unwrap_or_else(Utc::now);
        (end - self.started_at).num_seconds().max(0)
    }
}

struct RunningEntry {
    status: InstanceStatus,
    child: Option<Child>,
}

/// Manages many concurrent Minecraft JVM instances (player & bot accounts).
#[derive(Clone)]
pub struct InstanceManager {
    inner: Arc<RwLock<HashMap<String, RunningEntry>>>,
}

impl Default for InstanceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Snapshot status of all tracked instances.
    pub async fn list_statuses(&self) -> Vec<InstanceStatus> {
        let map = self.inner.read().await;
        map.values().map(|e| e.status.clone()).collect()
    }

    pub async fn get_status(&self, profile_id: &str) -> Option<InstanceStatus> {
        let map = self.inner.read().await;
        map.get(profile_id).map(|e| e.status.clone())
    }

    pub async fn is_running(&self, profile_id: &str) -> bool {
        let map = self.inner.read().await;
        matches!(
            map.get(profile_id).map(|e| &e.status.state),
            Some(InstanceState::Preparing | InstanceState::Running)
        )
    }

    pub async fn clear_logs(&self, profile_id: &str) {
        let mut map = self.inner.write().await;
        if let Some(entry) = map.get_mut(profile_id) {
            entry.status.logs.clear();
        }
    }

    /// Launch a single profile with the given account.
    pub async fn launch_profile(
        &self,
        paths: &AppPaths,
        settings: &LauncherSettings,
        profile: &Profile,
        account: &Account,
    ) -> Result<()> {
        if self.is_running(&profile.id).await {
            return Err(MubError::InstanceAlreadyRunning(profile.name.clone()));
        }

        // Insert Preparing state immediately so UI/CLI sees it right away.
        {
            let mut map = self.inner.write().await;
            map.insert(
                profile.id.clone(),
                RunningEntry {
                    status: InstanceStatus {
                        profile_id: profile.id.clone(),
                        profile_name: profile.name.clone(),
                        account_name: account.username.clone(),
                        pid: None,
                        state: InstanceState::Preparing,
                        progress: Some("Initializing...".to_string()),
                        started_at: Utc::now(),
                        ended_at: None,
                        logs: VecDeque::with_capacity(MAX_LOG_LINES),
                    },
                    child: None,
                },
            );
        }

        let natives_dir = paths.natives_dir(&profile.id);
        let game_dir = paths.instance_dir(&profile.id);
        if let Err(e) = std::fs::create_dir_all(&game_dir) {
            self.mark_failed(&profile.id, e.to_string()).await;
            return Err(e.into());
        }

        // Resolve Java path up front so NeoForge installer & JVM launch both use it.
        let java_bin = {
            let prof_java = profile.java_path.clone();
            let def_java = settings.default_java_path.clone();
            let mc_ver = profile.mc_version.clone();
            tokio::task::spawn_blocking(move || {
                crate::java::resolve_java_for_mc(prof_java.as_deref(), &def_java, &mc_ver)
            })
            .await
            .unwrap_or_else(|_| {
                profile
                    .java_path
                    .clone()
                    .unwrap_or_else(|| settings.default_java_path.clone())
            })
        };

        let inner_prog = Arc::clone(&self.inner);
        let pid_prog = profile.id.clone();
        let on_progress = move |msg: String| {
            let inner = Arc::clone(&inner_prog);
            let pid = pid_prog.clone();
            tokio::spawn(async move {
                let mut map = inner.write().await;
                if let Some(entry) = map.get_mut(&pid) {
                    entry.status.progress = Some(msg.clone());
                    if entry.status.logs.len() >= MAX_LOG_LINES {
                        entry.status.logs.pop_front();
                    }
                    entry.status.logs.push_back(format!("[MUB] {msg}"));
                }
            });
        };

        let prepared = match prepare_profile_version(
            paths,
            settings,
            profile,
            &java_bin,
            on_progress,
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                self.mark_failed(&profile.id, e.to_string()).await;
                return Err(e);
            }
        };

        let cp_sep = if cfg!(windows) { ";" } else { ":" };
        let classpath_str = prepared
            .classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(cp_sep);

        let mut cmd = Command::new(&java_bin);
        cmd.current_dir(&game_dir);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.arg(format!("-Xms{}M", profile.memory_min_mb));
        cmd.arg(format!("-Xmx{}M", profile.memory_max_mb));
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-Dminecraft.launcher.brand=ModsUndBots");
        cmd.arg("-Dminecraft.launcher.version=0.2.0");
        for extra in &prepared.extra_jvm_args {
            cmd.arg(extra);
        }
        for extra in &profile.jvm_args {
            cmd.arg(extra);
        }
        cmd.arg("-cp").arg(&classpath_str);
        cmd.arg(&prepared.main_class);

        // Minecraft game arguments
        cmd.arg("--username").arg(&account.username);
        cmd.arg("--version").arg(&prepared.version_id);
        cmd.arg("--gameDir").arg(game_dir.as_os_str());
        cmd.arg("--assetsDir").arg(paths.assets_dir().as_os_str());
        cmd.arg("--assetIndex").arg(&prepared.asset_index_id);
        cmd.arg("--uuid").arg(&account.mc_uuid);
        cmd.arg("--accessToken").arg(
            account
                .mc_access_token
                .as_deref()
                .unwrap_or("0"),
        );
        cmd.arg("--userType").arg(if account.is_msa() { "msa" } else { "legacy" });
        cmd.arg("--versionType").arg("ModsUndBots");

        for extra in &prepared.extra_game_args {
            cmd.arg(extra);
        }

        if let Some((w, h)) = profile.resolution {
            cmd.arg("--width").arg(w.to_string());
            cmd.arg("--height").arg(h.to_string());
        }

        if let Some(addr) = &profile.quick_connect {
            let trimmed = addr.trim();
            if !trimmed.is_empty() {
                let (host, port) = trimmed
                    .split_once(':')
                    .unwrap_or((trimmed, "25565"));
                if mc_supports_quick_play(&profile.mc_version) {
                    cmd.arg("--quickPlayMultiplayer")
                        .arg(format!("{host}:{port}"));
                } else {
                    cmd.arg("--server").arg(host).arg("--port").arg(port);
                }
            }
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let err = MubError::Launch(format!("Failed to spawn Java ({java_bin}): {e}"));
                self.mark_failed(&profile.id, err.to_string()).await;
                return Err(err);
            }
        };

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        {
            let mut map = self.inner.write().await;
            if let Some(entry) = map.get_mut(&profile.id) {
                entry.status.pid = pid;
                entry.status.state = InstanceState::Running;
                entry.status.progress = None;
                if entry.status.logs.len() >= MAX_LOG_LINES {
                    entry.status.logs.pop_front();
                }
                entry.status.logs.push_back(format!(
                    "[MUB] Launched '{}' (PID {:?}) using {}",
                    profile.name, pid, java_bin
                ));
                entry.child = Some(child);
            }
        }

        // Spawn log readers
        if let Some(out) = stdout {
            self.spawn_log_reader(profile.id.clone(), out);
        }
        if let Some(err) = stderr {
            self.spawn_log_reader(profile.id.clone(), err);
        }

        // Spawn exit watcher
        self.spawn_exit_watcher(profile.id.clone());

        Ok(())
    }

    /// Launch multiple profiles in sequence with a configurable stagger delay
    /// so CPU/disk/login spikes are avoided when booting a bot swarm.
    pub async fn launch_batch(
        &self,
        paths: &AppPaths,
        settings: &LauncherSettings,
        items: Vec<(Profile, Account)>,
    ) -> Vec<(String, Result<()>)> {
        self.launch_batch_with_stagger(paths, settings, items, None)
            .await
    }

    /// Launch multiple profiles in sequence with an optional stagger delay override.
    pub async fn launch_batch_with_stagger(
        &self,
        paths: &AppPaths,
        settings: &LauncherSettings,
        items: Vec<(Profile, Account)>,
        stagger_ms_override: Option<u64>,
    ) -> Vec<(String, Result<()>)> {
        let mut results = Vec::with_capacity(items.len());
        let stagger = Duration::from_millis(
            stagger_ms_override.unwrap_or(settings.batch_launch_stagger_ms),
        );

        for (idx, (profile, account)) in items.into_iter().enumerate() {
            if idx > 0 && !stagger.is_zero() {
                tokio::time::sleep(stagger).await;
            }
            let res = self
                .launch_profile(paths, settings, &profile, &account)
                .await;
            results.push((profile.id, res));
        }

        results
    }

    /// Terminate a running instance.
    pub async fn stop_profile(&self, profile_id: &str) -> Result<()> {
        let mut map = self.inner.write().await;
        let entry = map
            .get_mut(profile_id)
            .ok_or_else(|| MubError::InstanceNotRunning(profile_id.to_string()))?;

        if let Some(child) = entry.child.as_mut() {
            child.kill().await?;
            entry.status.state = InstanceState::Stopped { exit_code: None };
            entry.status.ended_at = Some(Utc::now());
            entry.status.pid = None;
        }
        Ok(())
    }

    /// Terminate all running instances.
    pub async fn stop_all(&self) -> Result<()> {
        let ids: Vec<String> = {
            let map = self.inner.read().await;
            map.keys().cloned().collect()
        };
        for id in ids {
            let _ = self.stop_profile(&id).await;
        }
        Ok(())
    }

    async fn mark_failed(&self, profile_id: &str, error: String) {
        let mut map = self.inner.write().await;
        if let Some(entry) = map.get_mut(profile_id) {
            entry.status.progress = None;
            entry.status.ended_at = Some(Utc::now());
            entry.status.state = InstanceState::Failed {
                error: error.clone(),
            };
            if entry.status.logs.len() >= MAX_LOG_LINES {
                entry.status.logs.pop_front();
            }
            entry.status.logs.push_back(format!("[MUB] ERROR: {error}"));
        }
    }

    fn spawn_log_reader<R>(&self, profile_id: String, reader: R)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut map = inner.write().await;
                if let Some(entry) = map.get_mut(&profile_id) {
                    if entry.status.logs.len() >= MAX_LOG_LINES {
                        entry.status.logs.pop_front();
                    }
                    entry.status.logs.push_back(line);
                } else {
                    break;
                }
            }
        });
    }

    fn spawn_exit_watcher(&self, profile_id: String) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let mut map = inner.write().await;
                let Some(entry) = map.get_mut(&profile_id) else {
                    break;
                };
                let Some(child) = entry.child.as_mut() else {
                    break;
                };
                match child.try_wait() {
                    Ok(Some(status)) => {
                        entry.status.state = InstanceState::Stopped {
                            exit_code: status.code(),
                        };
                        entry.status.ended_at = Some(Utc::now());
                        entry.status.pid = None;
                        entry.child = None;
                        break;
                    }
                    Ok(None) => continue,
                    Err(e) => {
                        entry.status.state = InstanceState::Failed {
                            error: e.to_string(),
                        };
                        entry.status.ended_at = Some(Utc::now());
                        entry.status.pid = None;
                        entry.child = None;
                        break;
                    }
                }
            }
        });
    }
}

/// Download and prepare a profile's Minecraft version, libraries, assets,
/// natives, and mod loader without spawning the JVM.
pub async fn prepare_profile_version<F>(
    paths: &AppPaths,
    settings: &LauncherSettings,
    profile: &Profile,
    java_bin: &str,
    on_progress: F,
) -> Result<PreparedVersion>
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    let natives_dir = paths.natives_dir(&profile.id);
    let game_dir = paths.instance_dir(&profile.id);
    std::fs::create_dir_all(&game_dir)?;

    let vm = VersionManager::new();
    match &profile.mod_loader {
        ModLoader::Vanilla => {
            vm.prepare_vanilla(
                paths,
                &profile.mc_version,
                &natives_dir,
                settings.max_concurrent_downloads,
                on_progress,
            )
            .await
        }
        ModLoader::Fabric { loader_version } => {
            vm.prepare_fabric(
                paths,
                &profile.mc_version,
                loader_version,
                &natives_dir,
                settings.max_concurrent_downloads,
                on_progress,
            )
            .await
        }
        ModLoader::Quilt { loader_version } => {
            vm.prepare_quilt(
                paths,
                &profile.mc_version,
                loader_version,
                &natives_dir,
                settings.max_concurrent_downloads,
                on_progress,
            )
            .await
        }
        ModLoader::NeoForge { loader_version } => {
            vm.prepare_neoforge(
                paths,
                &profile.mc_version,
                loader_version,
                &natives_dir,
                settings.max_concurrent_downloads,
                java_bin,
                on_progress,
            )
            .await
        }
        ModLoader::Forge { loader_version } => Err(MubError::Launch(format!(
            "Forge ({loader_version}) automated installer is not supported yet; consider NeoForge, Fabric, or Quilt, or install Forge manually."
        ))),
    }
}

fn mc_supports_quick_play(mc_version: &str) -> bool {
    let v = mc_version.trim();
    if let Some((year_str, rest)) = v.split_once('w')
        && year_str.len() == 2
        && let Ok(year) = year_str.parse::<u32>()
    {
        let week: u32 = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        return year > 23 || (year == 23 && week >= 14);
    }
    let core = v.split(['-', '+', ' ']).next().unwrap_or(v);
    let mut parts = core.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    major >= 2 || minor >= 20
}
