use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use mub_core::{
    Account, AccountRole, AccountStore, AppPaths, CatalogClient, CatalogSort, CatalogVersion,
    ContentKind, ContentSource, CurseForgeClient, InstanceManager, LauncherSettings, ModLoader,
    MsaAuthClient, MubError, Profile, ProfileKind, ProfileStore, SourceFilter,
    detect_java_installations, export_mrpack, import_curseforge_pack, import_mrpack,
    install_modpack_from_catalog, list_installed_content, remove_installed_content,
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
    /// Search, install, update, and manage mods & content from Modrinth / CurseForge
    Mods {
        #[command(subcommand)]
        action: ModsAction,
    },
    /// Import or export modpacks (.mrpack and CurseForge .zip)
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },
    /// Detect and inspect Java installations
    Java {
        #[command(subcommand)]
        action: JavaAction,
    },
}

#[derive(Subcommand)]
enum ModsAction {
    /// Search Modrinth and/or CurseForge for content
    Search {
        query: String,
        #[arg(long)]
        mc_version: Option<String>,
        #[arg(long)]
        fabric: bool,
        #[arg(long, value_enum, default_value_t = CliSource::All)]
        source: CliSource,
        #[arg(long, value_enum, default_value_t = CliContentKind::Mod)]
        kind: CliContentKind,
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Install content (by slug or project ID) with dependencies into a profile (or all bot profiles)
    Install {
        project: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        all_bots: bool,
        #[arg(long, value_enum, default_value_t = CliSource::All)]
        source: CliSource,
        #[arg(long, value_enum, default_value_t = CliContentKind::Mod)]
        kind: CliContentKind,
    },
    /// Check for and apply content updates in a profile
    Update {
        profile: String,
        #[arg(long, value_enum)]
        kind: Option<CliContentKind>,
    },
    /// List installed content in a profile
    List {
        profile: String,
        #[arg(long, value_enum, default_value_t = CliContentKind::Mod)]
        kind: CliContentKind,
    },
    /// Remove installed content from a profile
    Remove {
        profile: String,
        filename: String,
        #[arg(long, value_enum, default_value_t = CliContentKind::Mod)]
        kind: CliContentKind,
    },
}

#[derive(Subcommand)]
enum PackAction {
    /// Import a .mrpack or CurseForge .zip modpack into a new profile
    Import {
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum, default_value_t = CliKind::Player)]
        kind: CliKind,
    },
    /// Export a profile and its tracked content/overrides to a .mrpack archive
    Export {
        profile: String,
        output: PathBuf,
        #[arg(long, default_value = "1.0.0")]
        version: String,
        #[arg(long)]
        summary: Option<String>,
    },
}

#[derive(Subcommand)]
enum JavaAction {
    /// Detect Java installations on this system
    Detect,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliSource {
    All,
    Modrinth,
    Curseforge,
}

impl From<CliSource> for SourceFilter {
    fn from(s: CliSource) -> Self {
        match s {
            CliSource::All => SourceFilter::All,
            CliSource::Modrinth => SourceFilter::Modrinth,
            CliSource::Curseforge => SourceFilter::CurseForge,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CliContentKind {
    Mod,
    Modpack,
    Resourcepack,
    Shader,
    Datapack,
}

impl From<CliContentKind> for ContentKind {
    fn from(k: CliContentKind) -> Self {
        match k {
            CliContentKind::Mod => ContentKind::Mod,
            CliContentKind::Modpack => ContentKind::Modpack,
            CliContentKind::Resourcepack => ContentKind::ResourcePack,
            CliContentKind::Shader => ContentKind::Shader,
            CliContentKind::Datapack => ContentKind::DataPack,
        }
    }
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
    /// Add a Microsoft account via OAuth2 (Live browser or Device Code flow)
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
        #[arg(long, group = "loader")]
        fabric: Option<String>,
        #[arg(long, group = "loader")]
        quilt: Option<String>,
        #[arg(long, group = "loader")]
        neoforge: Option<String>,
        #[arg(long, group = "loader")]
        forge: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        server: Option<String>,
    },
    /// Clone an existing profile's configuration under a new name
    Clone {
        source: String,
        new_name: String,
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
        Commands::Mods { action } => handle_mods(action, &paths, &settings).await?,
        Commands::Pack { action } => handle_pack(action, &paths, &settings).await?,
        Commands::Java { action } => handle_java(action),
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
            let acc = if client.uses_live_flow() {
                println!(
                    "Open this URL in your browser and sign in:\n{}",
                    MsaAuthClient::live_authorize_url()
                );
                print!("Paste the final redirect URL (or ?code=...) here: ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                client.exchange_live_code(&input, role.into()).await?
            } else {
                let dev = client.request_device_code().await?;
                println!(
                    "Open {} and enter code: {}",
                    dev.verification_uri, dev.user_code
                );
                client.poll_device_code(&dev, role.into()).await?
            };
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
                    let group_suffix = p
                        .group
                        .as_deref()
                        .map(|g| format!(" | group: {g}"))
                        .unwrap_or_default();
                    println!(
                        "{} | {} | {} | {} ({}) | account: {}{}",
                        p.id, p.name, p.kind, p.mc_version, p.mod_loader, acc_name, group_suffix
                    );
                }
            }
        }
        ProfileAction::Create {
            name,
            kind,
            mc_version,
            fabric,
            quilt,
            neoforge,
            forge,
            group,
            account,
            server,
        } => {
            let loader = if let Some(v) = fabric {
                ModLoader::Fabric { loader_version: v }
            } else if let Some(v) = quilt {
                ModLoader::Quilt { loader_version: v }
            } else if let Some(v) = neoforge {
                ModLoader::NeoForge { loader_version: v }
            } else if let Some(v) = forge {
                ModLoader::Forge { loader_version: v }
            } else {
                ModLoader::Vanilla
            };
            let mut profile = Profile::new(name, kind.into(), mc_version, loader);
            if let Some(acc_q) = account {
                let acc = acc_store
                    .get(&acc_q)
                    .or_else(|| acc_store.find_by_username(&acc_q))
                    .with_context(|| format!("Account '{acc_q}' not found"))?;
                profile.account_id = Some(acc.id.clone());
            }
            profile.group = group;
            profile.quick_connect = server;
            println!("Created profile {} ({})", profile.name, profile.id);
            store.upsert(profile);
            store.save(&paths.profiles_file())?;
        }
        ProfileAction::Clone { source, new_name } => {
            let src = store
                .get(&source)
                .or_else(|| store.find_by_name(&source))
                .with_context(|| format!("Profile '{source}' not found"))?
                .clone();
            let cloned = src.clone_with_name(new_name);
            println!(
                "Cloned '{}' into '{}' ({})",
                src.name, cloned.name, cloned.id
            );
            store.upsert(cloned);
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

async fn resolve_catalog_version(
    catalog: &CatalogClient,
    source_filter: SourceFilter,
    kind: ContentKind,
    project: &str,
    mc_version: Option<&str>,
    loader: Option<&ModLoader>,
) -> Result<CatalogVersion> {
    match source_filter {
        SourceFilter::Modrinth => {
            let versions = catalog
                .list_versions(ContentSource::Modrinth, project, kind, mc_version, loader)
                .await?;
            versions
                .into_iter()
                .next()
                .with_context(|| format!("No compatible Modrinth version found for '{project}'"))
        }
        SourceFilter::CurseForge => {
            let cf_id = resolve_curseforge_project_id(catalog, kind, project, mc_version, loader).await?;
            let versions = catalog
                .list_versions(ContentSource::CurseForge, &cf_id, kind, mc_version, loader)
                .await?;
            versions
                .into_iter()
                .next()
                .with_context(|| format!("No compatible CurseForge version found for '{project}'"))
        }
        SourceFilter::All => {
            let is_numeric = project.chars().all(|c| c.is_ascii_digit()) && !project.is_empty();
            if is_numeric
                && catalog.has_curseforge()
                && let Ok(versions) = catalog
                    .list_versions(ContentSource::CurseForge, project, kind, mc_version, loader)
                    .await
                && let Some(first) = versions.into_iter().next()
            {
                return Ok(first);
            }
            if let Ok(versions) = catalog
                .list_versions(ContentSource::Modrinth, project, kind, mc_version, loader)
                .await
                && let Some(first) = versions.into_iter().next()
            {
                return Ok(first);
            }
            if catalog.has_curseforge() {
                let cf_id =
                    resolve_curseforge_project_id(catalog, kind, project, mc_version, loader).await?;
                let versions = catalog
                    .list_versions(ContentSource::CurseForge, &cf_id, kind, mc_version, loader)
                    .await?;
                if let Some(first) = versions.into_iter().next() {
                    return Ok(first);
                }
            }
            bail!("No compatible version found for '{project}'");
        }
    }
}

async fn resolve_curseforge_project_id(
    catalog: &CatalogClient,
    kind: ContentKind,
    project: &str,
    mc_version: Option<&str>,
    loader: Option<&ModLoader>,
) -> Result<String> {
    if project.chars().all(|c| c.is_ascii_digit()) && !project.is_empty() {
        return Ok(project.to_string());
    }
    let resp = catalog
        .search(
            SourceFilter::CurseForge,
            kind,
            project,
            mc_version,
            loader,
            &[],
            CatalogSort::Relevance,
            0,
            10,
        )
        .await?;
    let hit = resp
        .hits
        .iter()
        .find(|h| h.slug.eq_ignore_ascii_case(project))
        .or_else(|| resp.hits.first())
        .with_context(|| format!("CurseForge project '{project}' not found"))?;
    hit.curseforge_id
        .map(|id| id.to_string())
        .with_context(|| format!("CurseForge hit '{}' missing project ID", hit.title))
}

async fn handle_mods(
    action: ModsAction,
    paths: &AppPaths,
    settings: &LauncherSettings,
) -> Result<()> {
    let mut prof_store = ProfileStore::load(&paths.profiles_file())?;
    let catalog = CatalogClient::new(settings.effective_curseforge_api_key());

    match action {
        ModsAction::Search {
            query,
            mc_version,
            fabric,
            source,
            kind,
            limit,
        } => {
            let loader = fabric.then(|| ModLoader::Fabric {
                loader_version: String::new(),
            });
            let resp = catalog
                .search(
                    source.into(),
                    kind.into(),
                    &query,
                    mc_version.as_deref(),
                    loader.as_ref(),
                    &[],
                    CatalogSort::Relevance,
                    0,
                    limit,
                )
                .await?;
            if resp.hits.is_empty() {
                println!("No results found.");
            } else {
                for h in resp.hits {
                    let sources_str: Vec<String> =
                        h.sources.iter().map(|s| s.to_string()).collect();
                    let ids = match (&h.modrinth_id, h.curseforge_id) {
                        (Some(mr), Some(cf)) => format!("mr:{mr}, cf:{cf}"),
                        (Some(mr), None) => format!("mr:{mr}"),
                        (None, Some(cf)) => format!("cf:{cf}"),
                        (None, None) => h.slug.clone(),
                    };
                    println!(
                        "{} ({}) [{} | {}] by {} | {} downloads\n  {}",
                        h.title,
                        h.slug,
                        sources_str.join("+"),
                        ids,
                        h.author,
                        h.downloads,
                        h.description
                    );
                }
            }
        }
        ModsAction::Install {
            project,
            profile,
            all_bots,
            source,
            kind,
        } => {
            let content_kind: ContentKind = kind.into();
            if content_kind == ContentKind::Modpack {
                let pack_kind = if all_bots {
                    ProfileKind::Bot
                } else {
                    ProfileKind::Player
                };
                let latest = resolve_catalog_version(
                    &catalog,
                    source.into(),
                    content_kind,
                    &project,
                    None,
                    None,
                )
                .await?;
                let res = install_modpack_from_catalog(
                    paths,
                    &catalog,
                    &latest,
                    profile.as_deref(),
                    pack_kind,
                    settings.max_concurrent_downloads,
                    |msg| println!("{msg}"),
                )
                .await?;
                println!(
                    "Installed modpack '{}' ({}) — {} file(s), {} override(s).",
                    res.profile.name, res.profile.id, res.downloaded_files, res.extracted_overrides
                );
                for rf in &res.restricted_files {
                    eprintln!(
                        "Warning: '{}' has third-party distribution disabled; download manually at {}",
                        rf.filename, rf.website_url
                    );
                }
                prof_store.upsert(res.profile);
                prof_store.save(&paths.profiles_file())?;
                return Ok(());
            }

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
            let latest = resolve_catalog_version(
                &catalog,
                source.into(),
                content_kind,
                &project,
                Some(&reference.mc_version),
                Some(&reference.mod_loader),
            )
            .await?;

            let ids: Vec<String> = targets.iter().map(|p| p.id.clone()).collect();
            let installed = catalog
                .install_with_dependencies(
                    paths,
                    &ids,
                    content_kind,
                    &latest,
                    Some(&reference.mc_version),
                    Some(&reference.mod_loader),
                )
                .await?;
            println!(
                "Installed {} ({}) [+{} dependencies] to {} profile(s): {}",
                installed.first().map(String::as_str).unwrap_or(&project),
                latest.version_number,
                installed.len().saturating_sub(1),
                ids.len(),
                installed.join(", ")
            );
        }
        ModsAction::Update { profile, kind } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            let kinds: Vec<ContentKind> = match kind {
                Some(k) => vec![k.into()],
                None => vec![
                    ContentKind::Mod,
                    ContentKind::ResourcePack,
                    ContentKind::Shader,
                    ContentKind::DataPack,
                ],
            };

            let mut total_updates = 0usize;
            for k in kinds {
                let updates = catalog
                    .identify_and_check_updates(paths, &p.id, k, &p.mc_version, &p.mod_loader)
                    .await?;
                if updates.is_empty() {
                    continue;
                }
                for u in updates {
                    match catalog
                        .update_all_content(
                            paths,
                            &p.id,
                            std::slice::from_ref(&u),
                            Some(&p.mc_version),
                            Some(&p.mod_loader),
                        )
                        .await
                    {
                        Ok(updated_names) => {
                            total_updates += updated_names.len();
                            for name in updated_names {
                                println!(
                                    "[{}] Updated {} ({} -> {})",
                                    u.source,
                                    name,
                                    u.current_version,
                                    u.latest_version.version_number
                                );
                            }
                        }
                        Err(MubError::CurseForgeDistributionRestricted {
                            filename,
                            website_url,
                        }) => {
                            eprintln!(
                                "Warning: '{filename}' has third-party distribution disabled; download manually at {website_url}"
                            );
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            if total_updates == 0 {
                println!("All content in '{}' is up to date.", p.name);
            }
        }
        ModsAction::List { profile, kind } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            let content_kind: ContentKind = kind.into();
            let items = list_installed_content(paths, &p.id, content_kind)?;
            if items.is_empty() {
                println!("No {} installed in '{}'.", content_kind.label(), p.name);
            } else {
                for m in items {
                    let state = if m.enabled { "enabled" } else { "disabled" };
                    println!(
                        "{} ({:.1} KB, {})",
                        m.filename,
                        m.size_bytes as f64 / 1024.0,
                        state
                    );
                }
            }
        }
        ModsAction::Remove {
            profile,
            filename,
            kind,
        } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            remove_installed_content(paths, &p.id, kind.into(), &filename)?;
            println!("Removed '{}' from '{}'.", filename, p.name);
        }
    }

    Ok(())
}

async fn handle_pack(
    action: PackAction,
    paths: &AppPaths,
    settings: &LauncherSettings,
) -> Result<()> {
    let mut prof_store = ProfileStore::load(&paths.profiles_file())?;

    match action {
        PackAction::Import { file, name, kind } => {
            if !file.is_file() {
                bail!("Modpack file '{}' does not exist", file.display());
            }
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();

            let res = match ext.as_str() {
                "mrpack" => {
                    import_mrpack(
                        paths,
                        &file,
                        name.as_deref(),
                        kind.into(),
                        settings.max_concurrent_downloads,
                        |msg| println!("{msg}"),
                    )
                    .await?
                }
                "zip" => {
                    let cf_key = settings.effective_curseforge_api_key().context(
                        "Importing a CurseForge .zip pack requires a CurseForge API key (set curseforge_api_key in settings.json or MUB_CURSEFORGE_API_KEY).",
                    )?;
                    let cf_client = CurseForgeClient::new(cf_key);
                    import_curseforge_pack(
                        paths,
                        &file,
                        &cf_client,
                        name.as_deref(),
                        kind.into(),
                        settings.max_concurrent_downloads,
                        |msg| println!("{msg}"),
                    )
                    .await?
                }
                _ => bail!(
                    "Unsupported modpack file '{}': expected .mrpack or .zip",
                    file.display()
                ),
            };

            println!(
                "Imported '{}' ({}) — {} file(s) downloaded, {} override(s) extracted.",
                res.profile.name, res.profile.id, res.downloaded_files, res.extracted_overrides
            );
            for rf in &res.restricted_files {
                eprintln!(
                    "Warning: '{}' has third-party distribution disabled; download manually at {}",
                    rf.filename, rf.website_url
                );
            }
            prof_store.upsert(res.profile);
            prof_store.save(&paths.profiles_file())?;
        }
        PackAction::Export {
            profile,
            output,
            version,
            summary,
        } => {
            let p = prof_store
                .get(&profile)
                .or_else(|| prof_store.find_by_name(&profile))
                .with_context(|| format!("Profile '{profile}' not found"))?;
            export_mrpack(paths, p, &output, &version, summary.as_deref())?;
            println!("Exported '{}' to {}", p.name, output.display());
        }
    }

    Ok(())
}

fn handle_java(action: JavaAction) {
    match action {
        JavaAction::Detect => {
            let installs = detect_java_installations();
            if installs.is_empty() {
                println!("No Java installations detected.");
            } else {
                for j in installs {
                    println!(
                        "Java {} ({}) | {} | {}",
                        j.major,
                        j.version,
                        j.vendor,
                        j.path.display()
                    );
                }
            }
        }
    }
}
