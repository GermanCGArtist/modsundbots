use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use mub_core::{
    Account, AccountRole, AccountStore, AppPaths, InstanceManager, LauncherSettings, ModLoader,
    ModrinthClient, ModrinthSort, MsaAuthClient, Profile, ProfileKind, ProfileStore,
    list_installed_mods, remove_installed_mod,
};

#[derive(Parser)]
#[command(name = "mub", about = "ModsUndBots CLI — multi-instance & bot launcher")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage Minecraft accounts
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Manage instance profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Launch one or more profiles and wait until they exit
    Launch {
        /// Profile names or IDs to launch
        #[arg(required = true)]
        profiles: Vec<String>,
    },
    /// Launch all profiles matching a kind (player or bot)
    LaunchAll {
        #[arg(value_enum)]
        kind: CliKind,
    },
    /// Search and install mods from Modrinth
    Mods {
        #[command(subcommand)]
        action: ModsAction,
    },
}

#[derive(Subcommand)]
enum ModsAction {
    /// Search Modrinth for mods
    Search {
        query: String,
        #[arg(long)]
        mc_version: Option<String>,
        #[arg(long)]
        fabric: bool,
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Install a Modrinth mod (by slug or project ID) into a profile (or all bot profiles)
    Install {
        project: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        all_bots: bool,
    },
    /// List installed .jar mods in a profile
    List {
        profile: String,
    },
    /// Remove an installed .jar mod from a profile
    Remove {
        profile: String,
        filename: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CliRole {
    Player,
    Bot,
}

impl From<CliRole> for AccountRole {
    fn from(r: CliRole) -> Self {
        match r {
            CliRole::Player => AccountRole::Player,
            CliRole::Bot => AccountRole::Bot,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CliKind {
    Player,
    Bot,
}

impl From<CliKind> for ProfileKind {
    fn from(k: CliKind) -> Self {
        match k {
            CliKind::Player => ProfileKind::Player,
            CliKind::Bot => ProfileKind::Bot,
        }
    }
}

#[derive(Subcommand)]
enum AccountAction {
    List,
    /// Add a Microsoft account via OAuth2 Device Code flow
    AddMsa {
        #[arg(long, value_enum, default_value_t = CliRole::Player)]
        role: CliRole,
    },
    /// Add an offline/local bot account
    AddOffline {
        username: String,
        #[arg(long, value_enum, default_value_t = CliRole::Bot)]
        role: CliRole,
    },
    Refresh {
        username_or_id: String,
    },
    Remove {
        username_or_id: String,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    List,
    Create {
        name: String,
        #[arg(long, value_enum, default_value_t = CliKind::Bot)]
        kind: CliKind,
        #[arg(long, default_value = "1.21.4")]
        mc_version: String,
        #[arg(long)]
        fabric: Option<String>,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        server: Option<String>,
    },
    Remove {
        name_or_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let paths = AppPaths::default_root()?;
    paths.ensure_dirs()?;
    let settings = LauncherSettings::load(&paths.settings_file())?;

    match cli.command {
        Commands::Account { action } => handle_account(action, &paths, &settings).await?,
        Commands::Profile { action } => handle_profile(action, &paths)?,
        Commands::Launch { profiles } => {
            let mut acc_store = AccountStore::load(&paths.accounts_file())?;
            let prof_store = ProfileStore::load(&paths.profiles_file())?;
            let mut to_launch = Vec::new();
            for query in profiles {
                let p = prof_store
                    .get(&query)
                    .or_else(|| prof_store.find_by_name(&query))
                    .with_context(|| format!("Profile '{query}' not found"))?
                    .clone();
                let acc = resolve_account(&p, &mut acc_store, &settings, &paths).await?;
                to_launch.push((p, acc));
            }
            run_and_wait(&paths, &settings, to_launch).await?;
        }
        Commands::LaunchAll { kind } => {
            let mut acc_store = AccountStore::load(&paths.accounts_file())?;
            let prof_store = ProfileStore::load(&paths.profiles_file())?;
            let target_kind: ProfileKind = kind.into();
            let mut to_launch = Vec::new();
            for p in prof_store.profiles.iter().filter(|p| p.kind == target_kind) {
                let acc = resolve_account(p, &mut acc_store, &settings, &paths).await?;
                to_launch.push((p.clone(), acc));
            }
            if to_launch.is_empty() {
                println!("No {target_kind} profiles found.");
                return Ok(());
            }
            run_and_wait(&paths, &settings, to_launch).await?;
        }
        Commands::Mods { action } => handle_mods(action, &paths).await?,
    }

    Ok(())
}

async fn handle_account(
    action: AccountAction,
    paths: &AppPaths,
    settings: &LauncherSettings,
) -> Result<()> {
    let mut store = AccountStore::load(&paths.accounts_file())?;
    match action {
        AccountAction::List => {
            if store.accounts.is_empty() {
                println!("No accounts configured.");
            } else {
                for a in &store.accounts {
                    let kind = if a.is_msa() { "MSA" } else { "Offline" };
                    let valid = if a.is_msa() {
                        if a.is_token_valid() { "valid" } else { "needs refresh" }
                    } else {
                        "n/a"
                    };
                    println!("{} | {} | {} | {} | {}", a.id, a.username, a.role, kind, valid);
                }
            }
        }
        AccountAction::AddMsa { role } => {
            let client = MsaAuthClient::new(&settings.msa_client_id);
            let dev = client.request_device_code().await?;
            println!(
                "Open {} and enter code: {}",
                dev.verification_uri, dev.user_code
            );
            let acc = client.poll_device_code(&dev, role.into()).await?;
            println!("Signed in as {} ({})", acc.username, acc.mc_uuid);
            store.upsert(acc);
            store.save(&paths.accounts_file())?;
        }
        AccountAction::AddOffline { username, role } => {
            let acc = Account::new_offline(username, role.into());
            println!("Added offline account {} ({})", acc.username, acc.id);
            store.upsert(acc);
            store.save(&paths.accounts_file())?;
        }
        AccountAction::Refresh { username_or_id } => {
            let acc = store
                .get(&username_or_id)
                .or_else(|| store.find_by_username(&username_or_id))
                .with_context(|| format!("Account '{username_or_id}' not found"))?
                .clone();
            let client = MsaAuthClient::new(&settings.msa_client_id);
            let refreshed = client.refresh_account(&acc).await?;
            println!("Refreshed token for {}", refreshed.username);
            store.upsert(refreshed);
            store.save(&paths.accounts_file())?;
        }
        AccountAction::Remove { username_or_id } => {
            let id = store
                .get(&username_or_id)
                .or_else(|| store.find_by_username(&username_or_id))
                .map(|a| a.id.clone())
                .with_context(|| format!("Account '{username_or_id}' not found"))?;
            store.remove(&id);
            store.save(&paths.accounts_file())?;
            println!("Removed account {id}");
        }
    }
    Ok(())
}

fn handle_profile(action: ProfileAction, paths: &AppPaths) -> Result<()> {
    let mut store = ProfileStore::load(&paths.profiles_file())?;
    let acc_store = AccountStore::load(&paths.accounts_file())?;

    match action {
        ProfileAction::List => {
            if store.profiles.is_empty() {
                println!("No profiles configured.");
            } else {
                for p in &store.profiles {
                    let acc_name = p
                        .account_id
                        .as_deref()
                        .and_then(|id| acc_store.get(id))
                        .map(|a| a.username.as_str())
                        .unwrap_or("<unassigned>");
                    println!(
                        "{} | {} | {} | {} ({}) | account: {}",
                        p.id, p.name, p.kind, p.mc_version, p.mod_loader, acc_name
                    );
                }
            }
        }
        ProfileAction::Create {
            name,
            kind,
            mc_version,
            fabric,
            account,
            server,
        } => {
            let loader = match fabric {
                Some(v) => ModLoader::Fabric { loader_version: v },
                None => ModLoader::Vanilla,
            };
            let mut profile = Profile::new(name, kind.into(), mc_version, loader);
            if let Some(acc_q) = account {
                let acc = acc_store
                    .get(&acc_q)
                    .or_else(|| acc_store.find_by_username(&acc_q))
                    .with_context(|| format!("Account '{acc_q}' not found"))?;
                profile.account_id = Some(acc.id.clone());
            }
            profile.quick_connect = server;
            println!("Created profile {} ({})", profile.name, profile.id);
            store.upsert(profile);
            store.save(&paths.profiles_file())?;
        }
        ProfileAction::Remove { name_or_id } => {
            let id = store
                .get(&name_or_id)
                .or_else(|| store.find_by_name(&name_or_id))
                .map(|p| p.id.clone())
                .with_context(|| format!("Profile '{name_or_id}' not found"))?;
            store.remove(&id);
            store.save(&paths.profiles_file())?;
            println!("Removed profile {id}");
        }
    }
    Ok(())
}

async fn resolve_account(
    profile: &Profile,
    acc_store: &mut AccountStore,
    settings: &LauncherSettings,
    paths: &AppPaths,
) -> Result<Account> {
    let acc_id = profile
        .account_id
        .as_deref()
        .with_context(|| format!("Profile '{}' has no assigned account", profile.name))?;
    let acc = acc_store
        .get(acc_id)
        .with_context(|| format!("Assigned account '{acc_id}' not found"))?
        .clone();

    if acc.is_msa() && !acc.is_token_valid() {
        println!("Refreshing MSA token for {}...", acc.username);
        let client = MsaAuthClient::new(&settings.msa_client_id);
        let refreshed = client.refresh_account(&acc).await?;
        acc_store.upsert(refreshed.clone());
        acc_store.save(&paths.accounts_file())?;
        return Ok(refreshed);
    }
    if !acc.is_msa() && acc.mc_access_token.is_none() {
        return Ok(acc);
    }
    if acc.is_msa() && !acc.is_token_valid() {
        bail!("Unable to validate token for {}", acc.username);
    }
    Ok(acc)
}

async fn run_and_wait(
    paths: &AppPaths,
    settings: &LauncherSettings,
    items: Vec<(Profile, Account)>,
) -> Result<()> {
    let manager = InstanceManager::new();
    println!("Launching {} instance(s)...", items.len());
    let results = manager.launch_batch(paths, settings, items).await;
    for (id, res) in results {
        if let Err(e) = res {
            eprintln!("Failed to launch {id}: {e}");
        }
    }

    loop {
        let statuses = manager.list_statuses().await;
        let any_active = statuses.iter().any(|s| {
            matches!(
                s.state,
                mub_core::InstanceState::Preparing | mub_core::InstanceState::Running
            )
        });
        if !any_active {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    println!("All instances exited.");
    Ok(())
}

async fn handle_mods(action: ModsAction, paths: &AppPaths) -> Result<()> {
    let prof_store = ProfileStore::load(&paths.profiles_file())?;
    let client = ModrinthClient::new();

    match action {
        ModsAction::Search {
            query,
            mc_version,
            fabric,
            limit,
        } => {
            let loader = fabric.then(|| ModLoader::Fabric {
                loader_version: String::new(),
            });
            let hits = client
                .search_mods(
                    &query,
                    mc_version.as_deref(),
                    loader.as_ref(),
                    ModrinthSort::Relevance,
                    limit,
                )
                .await?;
            if hits.is_empty() {
                println!("No mods found on Modrinth.");
            } else {
                for h in hits {
                    println!(
                        "{} ({}) by {} | {} downloads\n  {}",
                        h.title, h.slug, h.author, h.downloads, h.description
                    );
                }
            }
        }
        ModsAction::Install {
            project,
            profile,
            all_bots,
        } => {
            let targets: Vec<&Profile> = if all_bots {
                prof_store
                    .profiles
                    .iter()
                    .filter(|p| p.kind == ProfileKind::Bot)
                    .collect()
            } else {
                let q = profile.context("Provide --profile <name> or --all-bots")?;
                let p = prof_store
                    .get(&q)
                    .or_else(|| prof_store.find_by_name(&q))
                    .with_context(|| format!("Profile '{q}' not found"))?;
                vec![p]
            };

            if targets.is_empty() {
                println!("No matching target profiles found.");
                return Ok(());
            }

            let reference = targets[0];
            let versions = client
                .list_project_versions(&project, Some(&reference.mc_version), Some(&reference.mod_loader))
                .await?;
            let latest = versions.first().with_context(|| {
                format!(
                    "No compatible version of '{project}' found for MC {} ({})",
                    reference.mc_version, reference.mod_loader
                )
            })?;

            let ids: Vec<String> = targets.iter().map(|p| p.id.clone()).collect();
            let filename = client
                .install_version_to_profiles(paths, &ids, latest)
                .await?;
            println!(
                "Installed {} ({}) to {} profile(s).",
                filename,
                latest.version_number,
                ids.len()
            );
        }
        ModsAction::List { profile } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            let mods = list_installed_mods(paths, &p.id)?;
            if mods.is_empty() {
                println!("No mods installed in '{}'.", p.name);
            } else {
                for m in mods {
                    println!("{} ({:.1} KB)", m.filename, m.size_bytes as f64 / 1024.0);
                }
            }
        }
        ModsAction::Remove { profile, filename } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            remove_installed_mod(paths, &p.id, &filename)?;
            println!("Removed '{}' from '{}'.", filename, p.name);
        }
    }

    Ok(())
}
