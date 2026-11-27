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
use crate::running::{RunningRecord, RunningStore};
use crate::window;

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
    /// The game window should be hidden (parked off-screen, see `window`).
    pub hide_window: bool,
    /// Re-attached after a mub restart: no stdout capture, logs are tailed from latest.log.
    pub adopted: bool,
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

fn push_log(status: &mut InstanceStatus, line: String) {
    if status.logs.len() >= MAX_LOG_LINES {
        status.logs.pop_front();
    }
    status.logs.push_back(line);
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
        // Atomically check running state and insert Preparing state so parallel
        // launches of the same profile cannot race and UI/CLI sees Preparing immediately.
        {
            let mut map = self.inner.write().await;
            if matches!(
                map.get(&profile.id).map(|e| &e.status.state),
                Some(InstanceState::Preparing | InstanceState::Running)
            ) {
                return Err(MubError::InstanceAlreadyRunning(profile.name.clone()));
            }
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
                        hide_window: profile.hides_window(),
                        adopted: false,
                    },
                    child: None,
                },
            );
        }

        let natives_dir = paths.natives_dir(&profile.id);
        let game_dir = paths.instance_dir(&profile.id);
        let tmp_dir = game_dir.join(".tmp");
        if let Err(e) = std::fs::create_dir_all(&game_dir).and_then(|_| std::fs::create_dir_all(&tmp_dir)) {
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
        let has_custom_tmpdir = prepared
            .extra_jvm_args
            .iter()
            .chain(profile.jvm_args.iter())
            .any(|a| a.starts_with("-Djava.io.tmpdir="));
        if !has_custom_tmpdir {
            cmd.arg(format!("-Djava.io.tmpdir={}", tmp_dir.display()));
        }
        for extra in &prepared.extra_jvm_args {
            if let Some(prop_key) = extra.strip_prefix("-D").and_then(|s| s.split_once('=').map(|(k, _)| k))
                && profile
                    .jvm_args
                    .iter()
                    .any(|u| u.starts_with(&format!("-D{prop_key}=")))
            {
                continue;
            }
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

        if profile.hides_window() {
            // Hidden bots should drop their frame rate when nobody plays them (vanilla AFK limiter:
            // 30 fps after 1 min, 10 fps after 10 min without real input). AFK is the default; only
            // repair a profile that was switched to "minimized".
            ensure_option(&game_dir.join("options.txt"), "inactivityFpsLimit", "\"afk\"");
            // The window shows for a moment before it is parked: never let it take the focus.
            cmd.env("SDL_WINDOW_ACTIVATE_WHEN_SHOWN", "0");
            cmd.env("SDL_WINDOW_ACTIVATE_WHEN_RAISED", "0");
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

        if let Some(p) = pid {
            RunningStore::upsert(
                &paths.running_file(),
                RunningRecord {
                    profile_id: profile.id.clone(),
                    profile_name: profile.name.clone(),
                    account_name: account.username.clone(),
                    pid: p,
                    start_time: window::process_start_time(p).unwrap_or(0),
                    started_at: Utc::now(),
                },
            );
            self.spawn_window_keeper(profile.id.clone(), p, false);
        }

        // Spawn exit watcher
        self.spawn_exit_watcher(profile.id.clone(), pid, paths.running_file());

        Ok(())
    }

    /// Launch multiple profiles in parallel (or with an optional stagger delay
    /// when configured in settings).
    pub async fn launch_batch(
        &self,
        paths: &AppPaths,
        settings: &LauncherSettings,
        items: Vec<(Profile, Account)>,
    ) -> Vec<(String, Result<()>)> {
        self.launch_batch_with_stagger(paths, settings, items, None)
            .await
    }

    /// Launch multiple profiles in parallel with an optional per-instance stagger override.
    ///
    /// When the effective stagger is 0 (the default), all profiles prepare and start
    /// concurrently. When a non-zero stagger is configured, tasks still run concurrently
    /// with an initial offset of `idx * stagger`.
    pub async fn launch_batch_with_stagger(
        &self,
        paths: &AppPaths,
        settings: &LauncherSettings,
        items: Vec<(Profile, Account)>,
        stagger_ms_override: Option<u64>,
    ) -> Vec<(String, Result<()>)> {
        let stagger = Duration::from_millis(
            stagger_ms_override.unwrap_or(settings.batch_launch_stagger_ms),
        );

        let futs = items.into_iter().enumerate().map(|(idx, (profile, account))| {
            let mgr = self.clone();
            let paths = paths.clone();
            let settings = settings.clone();
            async move {
                if idx > 0 && !stagger.is_zero() {
                    tokio::time::sleep(stagger.saturating_mul(idx as u32)).await;
                }
                let id = profile.id.clone();
                let res = mgr.launch_profile(&paths, &settings, &profile, &account).await;
                (id, res)
            }
        });

        futures_util::future::join_all(futs).await
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
        } else if entry.status.adopted
            && entry.status.state == InstanceState::Running
            && let Some(pid) = entry.status.pid
        {
            // Re-attached instance: no Child handle, end the process directly.
            window::terminate_process(pid);
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

    /// Hide or show an instance's game window; with `focus`, showing brings it to the front so you
    /// can play. Returns false when the instance has no window (yet).
    pub async fn set_window_hidden(&self, profile_id: &str, hidden: bool, focus: bool) -> bool {
        let pid = {
            let mut map = self.inner.write().await;
            let Some(entry) = map.get_mut(profile_id) else {
                return false;
            };
            entry.status.hide_window = hidden;
            entry.status.pid
        };
        let Some(pid) = pid else { return false };
        tokio::task::spawn_blocking(move || {
            if hidden {
                window::hide_pid(pid) > 0
            } else {
                window::show_pid(pid, focus) > 0
            }
        })
        .await
        .unwrap_or(false)
    }

    /// Re-attach to instances recorded in `running.json` that are still alive but not tracked here
    /// (mub-gui was closed or crashed while they ran). Returns how many were adopted.
    pub async fn adopt_running(&self, paths: &AppPaths) -> usize {
        let file = paths.running_file();
        let file2 = file.clone();
        let records = tokio::task::spawn_blocking(move || RunningStore::alive(&file2))
            .await
            .unwrap_or_default();
        let mut adopted = 0;
        for rec in records {
            {
                let mut map = self.inner.write().await;
                if matches!(
                    map.get(&rec.profile_id).map(|e| &e.status.state),
                    Some(InstanceState::Preparing | InstanceState::Running)
                ) {
                    continue;
                }
                let hidden = window::pid_hidden(rec.pid).unwrap_or(false);
                let mut status = InstanceStatus {
                    profile_id: rec.profile_id.clone(),
                    profile_name: rec.profile_name.clone(),
                    account_name: rec.account_name.clone(),
                    pid: Some(rec.pid),
                    state: InstanceState::Running,
                    progress: None,
                    started_at: rec.started_at,
                    ended_at: None,
                    logs: VecDeque::with_capacity(MAX_LOG_LINES),
                    hide_window: hidden,
                    adopted: true,
                };
                push_log(
                    &mut status,
                    format!(
                        "[MUB] Re-attached to running instance (PID {}); log continues from logs/latest.log",
                        rec.pid
                    ),
                );
                map.insert(rec.profile_id.clone(), RunningEntry { status, child: None });
            }
            self.spawn_window_keeper(rec.profile_id.clone(), rec.pid, true);
            self.spawn_adopted_watcher(
                rec.clone(),
                paths.logs_dir(&rec.profile_id).join("latest.log"),
                file.clone(),
            );
            adopted += 1;
        }
        adopted
    }

    /// Hides each new window of a hidden instance the moment it appears (often only after mod
    /// loading, and again if the game recreates it, e.g. on a fullscreen toggle). Every window is
    /// handled once: a window the user showed on purpose is never hidden again behind their back,
    /// even when another mub process (CLI, MCP) watches the same game. With `adopted`, the windows
    /// that already exist are left as they are.
    fn spawn_window_keeper(&self, profile_id: String, pid: u32, adopted: bool) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let mut seen: std::collections::HashSet<window::Hwnd> = if adopted {
                tokio::task::spawn_blocking(move || window::find_windows(pid))
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                Default::default()
            };
            loop {
                let fast = started.elapsed() < Duration::from_secs(180);
                tokio::time::sleep(Duration::from_millis(if fast { 100 } else { 1500 })).await;
                let want_hidden = {
                    let map = inner.read().await;
                    match map.get(&profile_id) {
                        Some(e)
                            if e.status.state == InstanceState::Running
                                && e.status.pid == Some(pid) =>
                        {
                            e.status.hide_window
                        }
                        _ => break,
                    }
                };
                let new: Vec<window::Hwnd> =
                    tokio::task::spawn_blocking(move || window::find_windows(pid))
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|h| !seen.contains(h))
                        .collect();
                if new.is_empty() {
                    continue;
                }
                seen.extend(new.iter().copied());
                if want_hidden {
                    let _ = tokio::task::spawn_blocking(move || {
                        for h in new {
                            window::set_hidden(h, true);
                        }
                    })
                    .await;
                }
            }
        });
    }

    /// For adopted instances: notices when the process ends and tails logs/latest.log into the log view.
    fn spawn_adopted_watcher(
        &self,
        rec: RunningRecord,
        log_file: std::path::PathBuf,
        running_file: std::path::PathBuf,
    ) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let mut offset = std::fs::metadata(&log_file).map(|m| m.len()).unwrap_or(0);
            // Start with the last lines so the log tab is not empty.
            if let Ok(text) = std::fs::read_to_string(&log_file) {
                let tail: Vec<String> = text.lines().rev().take(200).map(str::to_string).collect();
                let mut map = inner.write().await;
                if let Some(e) = map.get_mut(&rec.profile_id) {
                    for l in tail.into_iter().rev() {
                        push_log(&mut e.status, l);
                    }
                }
            }
            let mut partial = String::new();
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let rec2 = rec.clone();
                let alive = tokio::task::spawn_blocking(move || rec2.is_alive())
                    .await
                    .unwrap_or(false);
                let new_text = read_from(&log_file, &mut offset);
                let mut map = inner.write().await;
                let Some(e) = map.get_mut(&rec.profile_id) else { break };
                if !e.status.adopted {
                    break; // relaunched by this mub in the meantime
                }
                if let Some(t) = new_text {
                    partial.push_str(&t);
                    while let Some(i) = partial.find('\n') {
                        let line: String = partial.drain(..=i).collect();
                        push_log(&mut e.status, line.trim_end().to_string());
                    }
                }
                let stopped_here = e.status.state != InstanceState::Running;
                if !alive && !stopped_here {
                    e.status.state = InstanceState::Stopped { exit_code: None };
                    e.status.ended_at = Some(Utc::now());
                    e.status.pid = None;
                }
                if !alive || stopped_here {
                    drop(map);
                    RunningStore::remove(&running_file, &rec.profile_id, rec.pid);
                    break;
                }
            }
        });
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

    fn spawn_exit_watcher(&self, profile_id: String, pid: Option<u32>, running_file: std::path::PathBuf) {
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
                let res = child.try_wait();
                if !matches!(res, Ok(None))
                    && let Some(p) = pid
                {
                    RunningStore::remove(&running_file, &profile_id, p);
                }
                match res {
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

/// Read whatever was appended to `path` since `offset` (restarts at 0 when the file was rotated).
fn read_from(path: &std::path::Path, offset: &mut u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len < *offset {
        *offset = 0;
    }
    if len == *offset {
        return None;
    }
    f.seek(SeekFrom::Start(*offset)).ok()?;
    let mut buf = Vec::new();
    f.take(len - *offset).read_to_end(&mut buf).ok()?;
    *offset = len;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Set `key:value` in a Minecraft options.txt, but only when the file already exists (a missing file
/// is created by the game with its defaults) and the value differs.
fn ensure_option(path: &std::path::Path, key: &str, value: &str) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let prefix = format!("{key}:");
    let wanted = format!("{key}:{value}");
    let mut found = false;
    let mut changed = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                if l != wanted {
                    changed = true;
                    return wanted.clone();
                }
            }
            l.to_string()
        })
        .collect();
    if !found {
        lines.push(wanted);
        changed = true;
    }
    if changed {
        let _ = std::fs::write(path, lines.join("\n") + "\n");
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

#[cfg(test)]
mod window_option_tests {
    use super::*;

    #[test]
    fn ensure_option_replaces_appends_and_skips_missing_file() {
        let dir = std::env::temp_dir().join(format!("mub-opt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("options.txt");

        ensure_option(&f, "inactivityFpsLimit", "\"afk\"");
        assert!(!f.exists(), "a missing options.txt is left for the game to create");

        std::fs::write(&f, "version:4000\ninactivityFpsLimit:\"minimized\"\nmaxFps:120\n").unwrap();
        ensure_option(&f, "inactivityFpsLimit", "\"afk\"");
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(t.contains("inactivityFpsLimit:\"afk\"") && t.contains("maxFps:120") && !t.contains("minimized"));

        std::fs::write(&f, "version:4000\n").unwrap();
        ensure_option(&f, "inactivityFpsLimit", "\"afk\"");
        assert!(std::fs::read_to_string(&f).unwrap().ends_with("inactivityFpsLimit:\"afk\"\n"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bots_hide_by_default_players_do_not() {
        let mut bot = Profile::new("b", crate::profile::ProfileKind::Bot, "26.3", ModLoader::Vanilla);
        let player = Profile::new("p", crate::profile::ProfileKind::Player, "26.3", ModLoader::Vanilla);
        assert!(bot.hides_window());
        assert!(!player.hides_window());
        bot.hide_window = Some(false);
        assert!(!bot.hides_window());
        // Old profiles.json without the field still load (serde default).
        let mut v = serde_json::to_value(&player).unwrap();
        v.as_object_mut().unwrap().remove("hide_window");
        let back: Profile = serde_json::from_value(v).unwrap();
        assert_eq!(back.hide_window, None);
    }
}
