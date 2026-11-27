use std::io::Write;

use anyhow::Result;
use mub_core::{
    Account, AccountRole, AccountStore, AppPaths, BotCollection, BotCollectionStore,
    CatalogClient, CatalogSort, ContentKind, ContentSource, CurseForgeClient, InstanceManager,
    LauncherSettings, ModLoader, MsaAuthClient, Profile, ProfileKind, ProfileStore, SourceFilter,
    VersionManager, detect_java_installations, import_curseforge_pack, import_mrpack,
    list_installed_content, prepare_profile_version, remove_installed_content, resolve_java_for_mc,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

struct McpState {
    paths: AppPaths,
    manager: InstanceManager,
}

impl McpState {
    fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            manager: InstanceManager::new(),
        }
    }

    fn load_settings(&self) -> Result<LauncherSettings> {
        Ok(LauncherSettings::load(&self.paths.settings_file())?)
    }

    fn load_accounts(&self) -> Result<AccountStore> {
        Ok(AccountStore::load(&self.paths.accounts_file())?)
    }

    fn save_accounts(&self, store: &AccountStore) -> Result<()> {
        Ok(store.save(&self.paths.accounts_file())?)
    }

    fn load_profiles(&self) -> Result<ProfileStore> {
        Ok(ProfileStore::load(&self.paths.profiles_file())?)
    }

    fn save_profiles(&self, store: &ProfileStore) -> Result<()> {
        Ok(store.save(&self.paths.profiles_file())?)
    }

    fn load_collections(&self) -> Result<BotCollectionStore> {
        Ok(BotCollectionStore::load(&self.paths.collections_file())?)
    }

    fn save_collections(&self, store: &BotCollectionStore) -> Result<()> {
        Ok(store.save(&self.paths.collections_file())?)
    }
}

pub async fn run_mcp_server() -> Result<()> {
    let paths = AppPaths::default_root()?;
    paths.ensure_dirs()?;
    let state = McpState::new(paths);

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = std::io::stdout().lock();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                    }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_request(&state, req).await {
            writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

async fn handle_request(state: &McpState, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
    // Notifications have no id and expect no response.
    let id = match req.id {
        Some(id) => id,
        None => return None,
    };

    match req.method.as_str() {
        "initialize" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "modsundbots-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }),
        "ping" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({})),
            error: None,
        }),
        "tools/list" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({ "tools": tool_definitions() })),
            error: None,
        }),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            let call_res = call_tool(state, name, args).await;
            let result = match call_res {
                Ok(val) => {
                    let text = if let Some(s) = val.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                    };
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false
                    })
                }
                Err(e) => json!({
                    "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
                    "isError": true
                }),
            };

            Some(JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result),
                error: None,
            })
        }
        other => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {other}"),
            }),
        }),
    }
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_status_summary",
            "description": "Get ModsUndBots data root path, settings, account/profile/collection counts, and currently tracked running instances.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "detect_java",
            "description": "Detect installed Java runtimes on the system.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_mc_versions",
            "description": "List available Minecraft versions from Mojang.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "snapshots": { "type": "boolean", "description": "Include snapshot versions (default false)" },
                    "limit": { "type": "integer", "description": "Max versions to return (default 20)" }
                }
            }
        }),
        json!({
            "name": "list_loader_versions",
            "description": "List available mod loader versions (fabric, quilt, neoforge) for a Minecraft version.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mc_version": { "type": "string", "description": "Minecraft version, e.g. 1.21.4" },
                    "loader": { "type": "string", "enum": ["fabric", "quilt", "neoforge"], "description": "Mod loader" },
                    "limit": { "type": "integer", "description": "Max versions to return (default 15)" }
                },
                "required": ["mc_version", "loader"]
            }
        }),
        json!({
            "name": "list_accounts",
            "description": "List all configured Minecraft accounts (ID, username, UUID, role, type, token validity).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "role": { "type": "string", "enum": ["player", "bot"], "description": "Optional filter by role" }
                }
            }
        }),
        json!({
            "name": "add_offline_account",
            "description": "Create an offline/cracked Minecraft account (useful for offline-mode servers and local bot testing).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "Minecraft username" },
                    "role": { "type": "string", "enum": ["player", "bot"], "description": "Account role (default bot)" }
                },
                "required": ["username"]
            }
        }),
        json!({
            "name": "remove_account",
            "description": "Remove an account by ID or username.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account": { "type": "string", "description": "Account ID or username" }
                },
                "required": ["account"]
            }
        }),
        json!({
            "name": "start_msa_device_login",
            "description": "Start a Microsoft OAuth2 Device Code login flow. Returns verification_uri, user_code, and device_code. Show verification_uri and user_code to the user, then call complete_msa_device_login.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "complete_msa_device_login",
            "description": "Poll and complete a Microsoft OAuth2 Device Code login started with start_msa_device_login, and save the account.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device_code": { "type": "string", "description": "device_code returned by start_msa_device_login" },
                    "interval": { "type": "integer", "description": "Polling interval in seconds (default 5)" },
                    "expires_in": { "type": "integer", "description": "Expiry in seconds (default 900)" },
                    "role": { "type": "string", "enum": ["player", "bot"], "description": "Account role (default bot)" }
                },
                "required": ["device_code"]
            }
        }),
        json!({
            "name": "list_profiles",
            "description": "List all Minecraft instance profiles with assigned account, MC version, mod loader, group, and installed mod count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["player", "bot"], "description": "Optional filter by profile kind" },
                    "group": { "type": "string", "description": "Optional filter by group name" }
                }
            }
        }),
        json!({
            "name": "get_profile",
            "description": "Get detailed info about a single profile by ID or name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "create_profile",
            "description": "Create a new player or bot instance profile. If loader_version is omitted for fabric/quilt/neoforge, the latest stable version is selected automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Profile name" },
                    "kind": { "type": "string", "enum": ["player", "bot"], "description": "Profile kind (default bot)" },
                    "mc_version": { "type": "string", "description": "Minecraft version, e.g. 1.21.4" },
                    "loader": { "type": "string", "enum": ["vanilla", "fabric", "neoforge", "quilt", "forge"], "description": "Mod loader (default vanilla)" },
                    "loader_version": { "type": "string", "description": "Optional loader version (auto-picks latest if omitted)" },
                    "account": { "type": "string", "description": "Optional account ID or username to assign" },
                    "group": { "type": "string", "description": "Optional group label" },
                    "quick_connect": { "type": "string", "description": "Optional server host:port to auto-join on launch" },
                    "memory_min_mb": { "type": "integer", "description": "Optional minimum JVM heap MB" },
                    "memory_max_mb": { "type": "integer", "description": "Optional maximum JVM heap MB" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags" }
                },
                "required": ["name", "mc_version"]
            }
        }),
        json!({
            "name": "clone_profile",
            "description": "Clone an existing profile (and optionally its mods/config files) to a new profile, optionally assigning a different account.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_profile": { "type": "string", "description": "Source profile ID or name" },
                    "new_name": { "type": "string", "description": "Name for the cloned profile" },
                    "account": { "type": "string", "description": "Optional account ID or username to assign to the clone" },
                    "copy_files": { "type": "boolean", "description": "Copy mods and config directory from source profile (default true)" },
                    "group": { "type": "string", "description": "Optional override group" }
                },
                "required": ["source_profile", "new_name"]
            }
        }),
        json!({
            "name": "batch_create_bots",
            "description": "Create multiple bot instances at once (either from a template_profile or from mc_version + loader), automatically assigning distinct accounts, optionally adding them to a Bot Collection, and optionally preparing (downloading) their Minecraft/loader files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name_prefix": { "type": "string", "description": "Prefix for bot profile names, e.g. 'FarmBot' -> FarmBot-1, FarmBot-2..." },
                    "count": { "type": "integer", "description": "Number of bot instances to create (if omitted, uses length of accounts array)" },
                    "template_profile": { "type": "string", "description": "Optional existing profile ID or name to clone config/mods from" },
                    "mc_version": { "type": "string", "description": "Minecraft version (required if template_profile is not set)" },
                    "loader": { "type": "string", "enum": ["vanilla", "fabric", "neoforge", "quilt"], "description": "Mod loader (default vanilla if no template)" },
                    "loader_version": { "type": "string", "description": "Optional loader version (auto-picks latest if omitted)" },
                    "accounts": { "type": "array", "items": { "type": "string" }, "description": "Optional list of account IDs or usernames to assign 1-to-1" },
                    "auto_assign_bot_accounts": { "type": "boolean", "description": "If true and accounts is omitted, assign unassigned 'bot' accounts automatically (default true)" },
                    "quick_connect": { "type": "string", "description": "Optional server host:port to auto-join" },
                    "memory_max_mb": { "type": "integer", "description": "Optional max heap MB per bot" },
                    "collection_name": { "type": "string", "description": "Optional Bot Collection name to create/append these bots into" },
                    "prepare": { "type": "boolean", "description": "If true, download & prepare MC/loader files after creating (default false)" }
                },
                "required": ["name_prefix"]
            }
        }),
        json!({
            "name": "update_profile",
            "description": "Update an existing profile's settings (name, account, group, quick_connect, memory, jvm_args, tags, resolution).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "name": { "type": "string" },
                    "account": { "type": "string", "description": "Account ID or username (pass empty string to unassign)" },
                    "group": { "type": "string" },
                    "quick_connect": { "type": "string" },
                    "memory_min_mb": { "type": "integer" },
                    "memory_max_mb": { "type": "integer" },
                    "java_path": { "type": "string" },
                    "jvm_args": { "type": "array", "items": { "type": "string" } },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "hide_window": { "type": ["boolean", "null"], "description": "Hide the game window on launch (null = default: bots hidden, players shown)" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "assign_account",
            "description": "Assign an account (by ID or username) to a profile (by ID or name).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "account": { "type": "string", "description": "Account ID or username" }
                },
                "required": ["profile", "account"]
            }
        }),
        json!({
            "name": "delete_profile",
            "description": "Delete a profile (and optionally its instance directory on disk).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "delete_files": { "type": "boolean", "description": "Also delete the instance folder on disk (default false)" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "prepare_profile",
            "description": "Download and prepare a profile's Minecraft version, libraries, assets, natives, and mod loader without launching it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "search_mods",
            "description": "Search Modrinth and/or CurseForge for mods, modpacks, resourcepacks, shaders, or datapacks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "mc_version": { "type": "string" },
                    "loader": { "type": "string", "enum": ["fabric", "neoforge", "quilt", "forge"] },
                    "kind": { "type": "string", "enum": ["mod", "modpack", "resourcepack", "shader", "datapack"], "description": "Default mod" },
                    "source": { "type": "string", "enum": ["all", "modrinth", "curseforge"], "description": "Default all" },
                    "limit": { "type": "integer", "description": "Default 10" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "install_mod",
            "description": "Install a mod/content (by Modrinth slug/ID or CurseForge ID) with dependencies into a profile, all bot profiles, or a bot collection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Modrinth slug/ID or CurseForge project ID" },
                    "profile": { "type": "string", "description": "Target profile ID or name" },
                    "collection": { "type": "string", "description": "Target Bot Collection ID or name (installs into all members)" },
                    "all_bots": { "type": "boolean", "description": "Install into all bot profiles" },
                    "kind": { "type": "string", "enum": ["mod", "resourcepack", "shader", "datapack"], "description": "Default mod" },
                    "source": { "type": "string", "enum": ["all", "modrinth", "curseforge"], "description": "Default all" }
                },
                "required": ["project"]
            }
        }),
        json!({
            "name": "list_installed_mods",
            "description": "List installed mods/content in a profile.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "kind": { "type": "string", "enum": ["mod", "resourcepack", "shader", "datapack"], "description": "Default mod" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "remove_installed_mod",
            "description": "Remove an installed mod/content file from a profile.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "filename": { "type": "string", "description": "Filename (from list_installed_mods)" },
                    "kind": { "type": "string", "enum": ["mod", "resourcepack", "shader", "datapack"], "description": "Default mod" }
                },
                "required": ["profile", "filename"]
            }
        }),
        json!({
            "name": "import_modpack",
            "description": "Import a local .mrpack or CurseForge .zip modpack file into a new profile.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Path to .mrpack or CurseForge .zip file" },
                    "name": { "type": "string", "description": "Optional profile name override" },
                    "kind": { "type": "string", "enum": ["player", "bot"], "description": "Default bot" }
                },
                "required": ["file_path"]
            }
        }),
        json!({
            "name": "list_bot_collections",
            "description": "List all Bot Collections with their member profiles, assigned accounts, and running status.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "create_bot_collection",
            "description": "Create a new Bot Collection grouping multiple bot profiles for one-click 'Start All'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Collection name" },
                    "description": { "type": "string", "description": "Optional description" },
                    "profiles": { "type": "array", "items": { "type": "string" }, "description": "Profile IDs or names to include" },
                    "stagger_ms": { "type": "integer", "description": "Optional stagger delay in ms between launching bots" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "update_bot_collection",
            "description": "Update a Bot Collection (rename, description, stagger_ms, or add/remove/set profiles).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string", "description": "Collection ID or name" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "stagger_ms": { "type": "integer" },
                    "set_profiles": { "type": "array", "items": { "type": "string" }, "description": "Replace member list with these profile IDs/names" },
                    "add_profiles": { "type": "array", "items": { "type": "string" }, "description": "Add these profile IDs/names" },
                    "remove_profiles": { "type": "array", "items": { "type": "string" }, "description": "Remove these profile IDs/names" }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "delete_bot_collection",
            "description": "Delete a Bot Collection (does not delete the underlying profiles).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string", "description": "Collection ID or name" }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "prepare_bot_collection",
            "description": "Download and prepare Minecraft/loader files for all profiles in a Bot Collection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string", "description": "Collection ID or name" }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "start_bot_collection",
            "description": "Start all idle bot instances in a Bot Collection (staggered).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string", "description": "Collection ID or name" }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "launch_profiles",
            "description": "Launch one or more profiles by ID or name (staggered).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profiles": { "type": "array", "items": { "type": "string" }, "description": "Profile IDs or names" }
                },
                "required": ["profiles"]
            }
        }),
        json!({
            "name": "stop_instance",
            "description": "Stop a running profile instance by ID or name (or 'all' to stop all running instances).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID, name, or 'all'" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "set_window_hidden",
            "description": "Hide or show the game window of a running instance (started by any mub process). Showing brings it to the front.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "hidden": { "type": "boolean" }
                },
                "required": ["profile", "hidden"]
            }
        }),
        json!({
            "name": "list_running_instances",
            "description": "List all instances tracked in this MCP session with state, PID, and elapsed seconds.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "get_instance_logs",
            "description": "Get recent log lines from a tracked instance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile ID or name" },
                    "tail": { "type": "integer", "description": "Number of trailing log lines (default 80)" }
                },
                "required": ["profile"]
            }
        })
    ]
}

async fn call_tool(state: &McpState, name: &str, args: Value) -> Result<Value> {
    match name {
        "get_status_summary" => {
            let settings = state.load_settings()?;
            let accounts = state.load_accounts()?;
            let profiles = state.load_profiles()?;
            let collections = state.load_collections()?;
            let running = state.manager.list_statuses().await;
            Ok(json!({
                "data_root": state.paths.root(),
                "accounts_count": accounts.accounts.len(),
                "profiles_count": profiles.profiles.len(),
                "bot_profiles_count": profiles.profiles.iter().filter(|p| p.kind == ProfileKind::Bot).count(),
                "collections_count": collections.collections.len(),
                "tracked_instances": running.len(),
                "settings": {
                    "default_java_path": settings.default_java_path,
                    "batch_launch_stagger_ms": settings.batch_launch_stagger_ms,
                    "max_concurrent_downloads": settings.max_concurrent_downloads,
                }
            }))
        }
        "detect_java" => {
            let javas = tokio::task::spawn_blocking(detect_java_installations).await?;
            Ok(json!({ "installations": javas }))
        }
        "list_mc_versions" => {
            let snapshots = args.get("snapshots").and_then(|v| v.as_bool()).unwrap_or(false);
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let vm = VersionManager::new();
            let manifest = vm.fetch_manifest().await?;
            let versions: Vec<Value> = manifest
                .versions
                .into_iter()
                .filter(|v| snapshots || v.kind == "release")
                .take(limit)
                .map(|v| json!({ "id": v.id, "type": v.kind }))
                .collect();
            Ok(json!({ "latest_release": manifest.latest.release, "versions": versions }))
        }
        "list_loader_versions" => {
            let mc_version = req_str(&args, "mc_version")?;
            let loader = req_str(&args, "loader")?.to_lowercase();
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(15) as usize;
            let vm = VersionManager::new();
            let items: Vec<Value> = match loader.as_str() {
                "fabric" => vm
                    .fetch_fabric_loader_versions(&mc_version)
                    .await?
                    .into_iter()
                    .take(limit)
                    .map(|v| json!({ "version": v.version, "stable": v.stable }))
                    .collect(),
                "quilt" => vm
                    .fetch_quilt_loaders(&mc_version)
                    .await?
                    .into_iter()
                    .take(limit)
                    .map(|v| json!({ "version": v.version, "stable": v.stable }))
                    .collect(),
                "neoforge" => vm
                    .fetch_neoforge_versions(&mc_version)
                    .await?
                    .into_iter()
                    .take(limit)
                    .map(|v| json!({ "version": v.version, "stable": v.stable }))
                    .collect(),
                other => anyhow::bail!("Unsupported loader '{other}'. Use fabric, quilt, or neoforge."),
            };
            Ok(json!({ "mc_version": mc_version, "loader": loader, "versions": items }))
        }
        "list_accounts" => {
            let role_filter = args
                .get("role")
                .and_then(|v| v.as_str())
                .map(parse_role)
                .transpose()?;
            let store = state.load_accounts()?;
            let list: Vec<Value> = store
                .accounts
                .iter()
                .filter(|a| role_filter.is_none_or(|r| a.role == r))
                .map(|a| {
                    json!({
                        "id": a.id,
                        "username": a.username,
                        "mc_uuid": a.mc_uuid,
                        "role": a.role,
                        "type": if a.is_msa() { "msa" } else { "offline" },
                        "token_valid": a.is_token_valid(),
                    })
                })
                .collect();
            Ok(json!({ "accounts": list }))
        }
        "add_offline_account" => {
            let username = req_str(&args, "username")?;
            let role = args
                .get("role")
                .and_then(|v| v.as_str())
                .map(parse_role)
                .transpose()?
                .unwrap_or(AccountRole::Bot);
            let mut store = state.load_accounts()?;
            let acc = Account::new_offline(&username, role);
            let id = acc.id.clone();
            store.upsert(acc);
            state.save_accounts(&store)?;
            Ok(json!({ "id": id, "username": username, "role": role, "type": "offline" }))
        }
        "remove_account" => {
            let target = req_str(&args, "account")?;
            let mut store = state.load_accounts()?;
            let id = resolve_account(&store, &target)?.id.clone();
            let removed = store.remove(&id);
            state.save_accounts(&store)?;
            Ok(json!({ "removed": removed.map(|a| json!({ "id": a.id, "username": a.username })) }))
        }
        "start_msa_device_login" => {
            let settings = state.load_settings()?;
            let client = MsaAuthClient::new(&settings.msa_client_id);
            let dc = client.request_device_code().await?;
            Ok(json!({
                "verification_uri": dc.verification_uri,
                "user_code": dc.user_code,
                "device_code": dc.device_code,
                "expires_in": dc.expires_in,
                "interval": dc.interval,
                "message": dc.message
            }))
        }
        "complete_msa_device_login" => {
            let device_code = req_str(&args, "device_code")?;
            let interval = args.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);
            let expires_in = args.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);
            let role = args
                .get("role")
                .and_then(|v| v.as_str())
                .map(parse_role)
                .transpose()?
                .unwrap_or(AccountRole::Bot);
            let settings = state.load_settings()?;
            let client = MsaAuthClient::new(&settings.msa_client_id);
            let dev = mub_core::DeviceCodeResponse {
                device_code,
                user_code: String::new(),
                verification_uri: String::new(),
                expires_in,
                interval,
                message: None,
            };
            let acc = client.poll_device_code(&dev, role).await?;
            let mut store = state.load_accounts()?;
            let summary = json!({ "id": acc.id, "username": acc.username, "mc_uuid": acc.mc_uuid, "role": acc.role });
            store.upsert(acc);
            state.save_accounts(&store)?;
            Ok(summary)
        }
        "list_profiles" => {
            let kind_filter = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_kind)
                .transpose()?;
            let group_filter = args.get("group").and_then(|v| v.as_str());
            let profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let list: Vec<Value> = profiles
                .profiles
                .iter()
                .filter(|p| kind_filter.is_none_or(|k| p.kind == k))
                .filter(|p| {
                    group_filter.is_none_or(|g| {
                        p.group
                            .as_deref()
                            .is_some_and(|pg| pg.eq_ignore_ascii_case(g))
                    })
                })
                .map(|p| profile_summary(&state.paths, p, &accounts))
                .collect();
            Ok(json!({ "profiles": list }))
        }
        "get_profile" => {
            let target = req_str(&args, "profile")?;
            let profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let p = resolve_profile(&profiles, &target)?;
            let mods = list_installed_content(&state.paths, &p.id, ContentKind::Mod)
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.filename)
                .collect::<Vec<_>>();
            Ok(json!({
                "profile": profile_summary(&state.paths, p, &accounts),
                "instance_dir": state.paths.instance_dir(&p.id),
                "installed_mods": mods,
            }))
        }
        "create_profile" => {
            let name = req_str(&args, "name")?;
            let mc_version = req_str(&args, "mc_version")?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_kind)
                .transpose()?
                .unwrap_or(ProfileKind::Bot);
            let loader_name = args
                .get("loader")
                .and_then(|v| v.as_str())
                .unwrap_or("vanilla");
            let loader_ver = args.get("loader_version").and_then(|v| v.as_str());
            let mod_loader = resolve_mod_loader(loader_name, &mc_version, loader_ver).await?;

            let mut profiles = state.load_profiles()?;
            if profiles.find_by_name(&name).is_some() {
                anyhow::bail!("A profile named '{name}' already exists");
            }
            let mut profile = Profile::new(&name, kind, &mc_version, mod_loader);
            if let Some(acc_ref) = args.get("account").and_then(|v| v.as_str()) {
                let accounts = state.load_accounts()?;
                let acc = resolve_account(&accounts, acc_ref)?;
                profile.account_id = Some(acc.id.clone());
            }
            if let Some(g) = args.get("group").and_then(|v| v.as_str()) {
                profile.group = Some(g.to_string());
            }
            if let Some(qc) = args.get("quick_connect").and_then(|v| v.as_str()) {
                profile.quick_connect = Some(qc.to_string());
            }
            if let Some(min) = args.get("memory_min_mb").and_then(|v| v.as_u64()) {
                profile.memory_min_mb = min as u32;
            }
            if let Some(max) = args.get("memory_max_mb").and_then(|v| v.as_u64()) {
                profile.memory_max_mb = max as u32;
            }
            if let Some(tags) = args.get("tags").and_then(|v| v.as_array()) {
                profile.tags = tags
                    .iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect();
            }

            std::fs::create_dir_all(state.paths.instance_dir(&profile.id))?;
            let accounts = state.load_accounts()?;
            let summary = profile_summary(&state.paths, &profile, &accounts);
            profiles.upsert(profile);
            state.save_profiles(&profiles)?;
            Ok(summary)
        }
        "clone_profile" => {
            let src_ref = req_str(&args, "source_profile")?;
            let new_name = req_str(&args, "new_name")?;
            let copy_files = args
                .get("copy_files")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let mut profiles = state.load_profiles()?;
            if profiles.find_by_name(&new_name).is_some() {
                anyhow::bail!("A profile named '{new_name}' already exists");
            }
            let src = resolve_profile(&profiles, &src_ref)?.clone();
            let mut cloned = src.clone_with_name(&new_name);
            if let Some(acc_ref) = args.get("account").and_then(|v| v.as_str()) {
                let accounts = state.load_accounts()?;
                cloned.account_id = Some(resolve_account(&accounts, acc_ref)?.id.clone());
            }
            if let Some(g) = args.get("group").and_then(|v| v.as_str()) {
                cloned.group = Some(g.to_string());
            }
            std::fs::create_dir_all(state.paths.instance_dir(&cloned.id))?;
            if copy_files {
                copy_profile_content(&state.paths, &src.id, &cloned.id)?;
            }
            let accounts = state.load_accounts()?;
            let summary = profile_summary(&state.paths, &cloned, &accounts);
            profiles.upsert(cloned);
            state.save_profiles(&profiles)?;
            Ok(summary)
        }
        "batch_create_bots" => batch_create_bots(state, args).await,
        "update_profile" => {
            let target = req_str(&args, "profile")?;
            let mut profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let id = resolve_profile(&profiles, &target)?.id.clone();
            let p = profiles.get_mut(&id).unwrap();

            if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                p.name = n.to_string();
            }
            if let Some(acc_ref) = args.get("account").and_then(|v| v.as_str()) {
                if acc_ref.trim().is_empty() {
                    p.account_id = None;
                } else {
                    p.account_id = Some(resolve_account(&accounts, acc_ref)?.id.clone());
                }
            }
            if let Some(g) = args.get("group").and_then(|v| v.as_str()) {
                p.group = if g.trim().is_empty() {
                    None
                } else {
                    Some(g.to_string())
                };
            }
            if let Some(qc) = args.get("quick_connect").and_then(|v| v.as_str()) {
                p.quick_connect = if qc.trim().is_empty() {
                    None
                } else {
                    Some(qc.to_string())
                };
            }
            if let Some(v) = args.get("hide_window") {
                p.hide_window = v.as_bool();
            }
            if let Some(min) = args.get("memory_min_mb").and_then(|v| v.as_u64()) {
                p.memory_min_mb = min as u32;
            }
            if let Some(max) = args.get("memory_max_mb").and_then(|v| v.as_u64()) {
                p.memory_max_mb = max as u32;
            }
            if let Some(jp) = args.get("java_path").and_then(|v| v.as_str()) {
                p.java_path = if jp.trim().is_empty() {
                    None
                } else {
                    Some(jp.to_string())
                };
            }
            if let Some(jargs) = args.get("jvm_args").and_then(|v| v.as_array()) {
                p.jvm_args = jargs
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
            }
            if let Some(tags) = args.get("tags").and_then(|v| v.as_array()) {
                p.tags = tags
                    .iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect();
            }
            let summary = profile_summary(&state.paths, p, &accounts);
            state.save_profiles(&profiles)?;
            Ok(summary)
        }
        "assign_account" => {
            let prof_ref = req_str(&args, "profile")?;
            let acc_ref = req_str(&args, "account")?;
            let mut profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let acc = resolve_account(&accounts, &acc_ref)?;
            let pid = resolve_profile(&profiles, &prof_ref)?.id.clone();
            let p = profiles.get_mut(&pid).unwrap();
            p.account_id = Some(acc.id.clone());
            let summary = profile_summary(&state.paths, p, &accounts);
            state.save_profiles(&profiles)?;
            Ok(summary)
        }
        "delete_profile" => {
            let target = req_str(&args, "profile")?;
            let delete_files = args
                .get("delete_files")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut profiles = state.load_profiles()?;
            let id = resolve_profile(&profiles, &target)?.id.clone();
            let removed = profiles.remove(&id);
            state.save_profiles(&profiles)?;

            // Clean up from any collections too
            let mut collections = state.load_collections()?;
            for col in &mut collections.collections {
                col.remove_profile(&id);
            }
            state.save_collections(&collections)?;

            if delete_files {
                let dir = state.paths.instance_dir(&id);
                if dir.exists() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
            }
            Ok(json!({ "deleted": removed.map(|p| json!({ "id": p.id, "name": p.name })) }))
        }
        "prepare_profile" => {
            let target = req_str(&args, "profile")?;
            let profiles = state.load_profiles()?;
            let settings = state.load_settings()?;
            let profile = resolve_profile(&profiles, &target)?.clone();
            let java_bin = resolve_java_async(&settings, &profile).await;
            let prepared =
                prepare_profile_version(&state.paths, &settings, &profile, &java_bin, |_| {})
                    .await?;
            Ok(json!({
                "profile": profile.name,
                "version_id": prepared.version_id,
                "main_class": prepared.main_class,
                "java_bin": java_bin,
                "classpath_entries": prepared.classpath.len(),
            }))
        }
        "search_mods" => {
            let query = req_str(&args, "query")?;
            let mc_ver = args.get("mc_version").and_then(|v| v.as_str());
            let loader = args
                .get("loader")
                .and_then(|v| v.as_str())
                .and_then(|s| match s.to_lowercase().as_str() {
                    "fabric" => Some(ModLoader::Fabric {
                        loader_version: String::new(),
                    }),
                    "neoforge" => Some(ModLoader::NeoForge {
                        loader_version: String::new(),
                    }),
                    "quilt" => Some(ModLoader::Quilt {
                        loader_version: String::new(),
                    }),
                    "forge" => Some(ModLoader::Forge {
                        loader_version: String::new(),
                    }),
                    _ => None,
                });
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_content_kind)
                .transpose()?
                .unwrap_or(ContentKind::Mod);
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .map(parse_source_filter)
                .transpose()?
                .unwrap_or(SourceFilter::All);
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as u32;

            let settings = state.load_settings()?;
            let client = CatalogClient::with_github(
                settings.effective_curseforge_api_key(),
                settings.effective_github_update_token(),
                settings.github_mod_repos.clone(),
            );
            let resp = client
                .search(
                    source,
                    kind,
                    &query,
                    mc_ver,
                    loader.as_ref(),
                    &[],
                    CatalogSort::Downloads,
                    0,
                    limit,
                )
                .await?;
            let hits: Vec<Value> = resp
                .hits
                .into_iter()
                .map(|h| {
                    json!({
                        "sources": h.sources,
                        "modrinth_id": h.modrinth_id,
                        "curseforge_id": h.curseforge_id,
                        "slug": h.slug,
                        "title": h.title,
                        "author": h.author,
                        "downloads": h.downloads,
                        "description": h.description,
                    })
                })
                .collect();
            Ok(json!({ "total_hits": resp.total_hits, "hits": hits }))
        }
        "install_mod" => {
            let project = req_str(&args, "project")?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_content_kind)
                .transpose()?
                .unwrap_or(ContentKind::Mod);
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .map(parse_source_filter)
                .transpose()?
                .unwrap_or(SourceFilter::All);

            let profiles = state.load_profiles()?;
            let target_profiles: Vec<Profile> = if args
                .get("all_bots")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                profiles
                    .profiles
                    .iter()
                    .filter(|p| p.kind == ProfileKind::Bot)
                    .cloned()
                    .collect()
            } else if let Some(col_ref) = args.get("collection").and_then(|v| v.as_str()) {
                let cols = state.load_collections()?;
                let col = cols
                    .resolve(col_ref)
                    .ok_or_else(|| anyhow::anyhow!("Collection '{col_ref}' not found"))?;
                col.profile_ids
                    .iter()
                    .filter_map(|id| profiles.get(id).cloned())
                    .collect()
            } else if let Some(prof_ref) = args.get("profile").and_then(|v| v.as_str()) {
                vec![resolve_profile(&profiles, prof_ref)?.clone()]
            } else {
                anyhow::bail!("Specify one of: profile, collection, or all_bots: true");
            };

            if target_profiles.is_empty() {
                anyhow::bail!("No matching target profiles found");
            }

            let settings = state.load_settings()?;
            let client = CatalogClient::with_github(
                settings.effective_curseforge_api_key(),
                settings.effective_github_update_token(),
                settings.github_mod_repos.clone(),
            );
            let content_source = match source {
                SourceFilter::CurseForge => ContentSource::CurseForge,
                SourceFilter::Modrinth => ContentSource::Modrinth,
                SourceFilter::GitHub => ContentSource::GitHub,
                SourceFilter::All => {
                    if mub_core::github::parse_repo_spec(&project).is_some() {
                        ContentSource::GitHub
                    } else if project.chars().all(|c| c.is_ascii_digit()) && client.has_curseforge()
                    {
                        ContentSource::CurseForge
                    } else {
                        ContentSource::Modrinth
                    }
                }
            };

            let ref_prof = &target_profiles[0];
            let versions = client
                .list_versions(
                    content_source,
                    &project,
                    kind,
                    Some(&ref_prof.mc_version),
                    Some(&ref_prof.mod_loader),
                )
                .await?;
            let chosen = versions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("No compatible version found for '{project}'"))?;

            let ids: Vec<String> = target_profiles.iter().map(|p| p.id.clone()).collect();
            let installed = client
                .install_with_dependencies(
                    &state.paths,
                    &ids,
                    kind,
                    &chosen,
                    Some(&ref_prof.mc_version),
                    Some(&ref_prof.mod_loader),
                )
                .await?;
            Ok(json!({
                "installed_files": installed,
                "target_profiles": target_profiles.iter().map(|p| &p.name).collect::<Vec<_>>(),
            }))
        }
        "list_installed_mods" => {
            let target = req_str(&args, "profile")?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_content_kind)
                .transpose()?
                .unwrap_or(ContentKind::Mod);
            let profiles = state.load_profiles()?;
            let p = resolve_profile(&profiles, &target)?;
            let items = list_installed_content(&state.paths, &p.id, kind)?;
            let list: Vec<Value> = items
                .into_iter()
                .map(|i| {
                    json!({
                        "filename": i.filename,
                        "enabled": i.enabled,
                        "size_bytes": i.size_bytes,
                        "title": i.meta.as_ref().and_then(|m| m.title.clone()),
                        "version": i.meta.as_ref().map(|m| m.version_number.clone()),
                    })
                })
                .collect();
            Ok(json!({ "profile": p.name, "items": list }))
        }
        "remove_installed_mod" => {
            let target = req_str(&args, "profile")?;
            let filename = req_str(&args, "filename")?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_content_kind)
                .transpose()?
                .unwrap_or(ContentKind::Mod);
            let profiles = state.load_profiles()?;
            let p = resolve_profile(&profiles, &target)?;
            remove_installed_content(&state.paths, &p.id, kind, &filename)?;
            Ok(json!({ "removed": filename, "profile": p.name }))
        }
        "import_modpack" => {
            let file_path = std::path::PathBuf::from(req_str(&args, "file_path")?);
            let name = args.get("name").and_then(|v| v.as_str());
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(parse_kind)
                .transpose()?
                .unwrap_or(ProfileKind::Bot);
            let settings = state.load_settings()?;
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let res = if ext == "mrpack" {
                import_mrpack(
                    &state.paths,
                    &file_path,
                    name,
                    kind,
                    settings.max_concurrent_downloads,
                    |_| {},
                )
                .await?
            } else {
                let cf_key = settings
                    .effective_curseforge_api_key()
                    .ok_or_else(|| anyhow::anyhow!("CurseForge API key required for .zip packs"))?;
                let cf_client = CurseForgeClient::new(&cf_key);
                import_curseforge_pack(
                    &state.paths,
                    &file_path,
                    &cf_client,
                    name,
                    kind,
                    settings.max_concurrent_downloads,
                    |_| {},
                )
                .await?
            };
            let mut profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let summary = profile_summary(&state.paths, &res.profile, &accounts);
            profiles.upsert(res.profile);
            state.save_profiles(&profiles)?;
            Ok(json!({
                "profile": summary,
                "downloaded_files": res.downloaded_files,
                "extracted_overrides": res.extracted_overrides,
            }))
        }
        "list_bot_collections" => {
            let cols = state.load_collections()?;
            let profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let statuses = state.manager.list_statuses().await;
            let list: Vec<Value> = cols
                .collections
                .iter()
                .map(|c| collection_summary(c, &profiles, &accounts, &statuses))
                .collect();
            Ok(json!({ "collections": list }))
        }
        "create_bot_collection" => {
            let name = req_str(&args, "name")?;
            let mut cols = state.load_collections()?;
            if cols.find_by_name(&name).is_some() {
                anyhow::bail!("A bot collection named '{name}' already exists");
            }
            let profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let mut col = BotCollection::new(&name);
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                col.description = desc.to_string();
            }
            if let Some(stagger) = args.get("stagger_ms").and_then(|v| v.as_u64()) {
                col.stagger_ms = Some(stagger);
            }
            if let Some(arr) = args.get("profiles").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let p = resolve_profile(&profiles, s)?;
                        col.add_profile(&p.id);
                    }
                }
            }
            let statuses = state.manager.list_statuses().await;
            let summary = collection_summary(&col, &profiles, &accounts, &statuses);
            cols.upsert(col);
            state.save_collections(&cols)?;
            Ok(summary)
        }
        "update_bot_collection" => {
            let target = req_str(&args, "collection")?;
            let mut cols = state.load_collections()?;
            let profiles = state.load_profiles()?;
            let accounts = state.load_accounts()?;
            let col_id = cols
                .resolve(&target)
                .ok_or_else(|| anyhow::anyhow!("Collection '{target}' not found"))?
                .id
                .clone();
            let col = cols.get_mut(&col_id).unwrap();

            if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                col.name = n.to_string();
            }
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                col.description = desc.to_string();
            }
            if let Some(stagger) = args.get("stagger_ms").and_then(|v| v.as_u64()) {
                col.stagger_ms = Some(stagger);
            }
            if let Some(arr) = args.get("set_profiles").and_then(|v| v.as_array()) {
                col.profile_ids.clear();
                for item in arr {
                    if let Some(s) = item.as_str() {
                        col.add_profile(&resolve_profile(&profiles, s)?.id);
                    }
                }
            }
            if let Some(arr) = args.get("add_profiles").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        col.add_profile(&resolve_profile(&profiles, s)?.id);
                    }
                }
            }
            if let Some(arr) = args.get("remove_profiles").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let pid = resolve_profile(&profiles, s)?.id.clone();
                        col.remove_profile(&pid);
                    }
                }
            }
            let statuses = state.manager.list_statuses().await;
            let summary = collection_summary(col, &profiles, &accounts, &statuses);
            state.save_collections(&cols)?;
            Ok(summary)
        }
        "delete_bot_collection" => {
            let target = req_str(&args, "collection")?;
            let mut cols = state.load_collections()?;
            let id = cols
                .resolve(&target)
                .ok_or_else(|| anyhow::anyhow!("Collection '{target}' not found"))?
                .id
                .clone();
            let removed = cols.remove(&id);
            state.save_collections(&cols)?;
            Ok(json!({ "deleted": removed.map(|c| json!({ "id": c.id, "name": c.name })) }))
        }
        "prepare_bot_collection" => {
            let target = req_str(&args, "collection")?;
            let cols = state.load_collections()?;
            let profiles = state.load_profiles()?;
            let settings = state.load_settings()?;
            let col = cols
                .resolve(&target)
                .ok_or_else(|| anyhow::anyhow!("Collection '{target}' not found"))?;
            let mut join_set = tokio::task::JoinSet::new();
            for pid in &col.profile_ids {
                if let Some(prof) = profiles.get(pid).cloned() {
                    let paths = state.paths.clone();
                    let settings = settings.clone();
                    join_set.spawn(async move {
                        let java_bin = resolve_java_async(&settings, &prof).await;
                        match prepare_profile_version(&paths, &settings, &prof, &java_bin, |_| {})
                            .await
                        {
                            Ok(prep) => json!({
                                "profile": prof.name,
                                "status": "prepared",
                                "version_id": prep.version_id,
                            }),
                            Err(e) => json!({
                                "profile": prof.name,
                                "status": "failed",
                                "error": e.to_string(),
                            }),
                        }
                    });
                }
            }
            let mut results = Vec::new();
            while let Some(res) = join_set.join_next().await {
                if let Ok(item) = res {
                    results.push(item);
                }
            }
            Ok(json!({ "collection": col.name, "results": results }))
        }
        "start_bot_collection" => {
            let target = req_str(&args, "collection")?;
            let mut cols = state.load_collections()?;
            let col_id = cols
                .resolve(&target)
                .ok_or_else(|| anyhow::anyhow!("Collection '{target}' not found"))?
                .id
                .clone();
            let col = cols.get(&col_id).unwrap().clone();
            let profiles = state.load_profiles()?;
            let mut accounts = state.load_accounts()?;
            let settings = state.load_settings()?;

            let mut items = Vec::new();
            let mut skipped = Vec::new();
            for pid in &col.profile_ids {
                let Some(prof) = profiles.get(pid) else {
                    skipped.push(json!({ "profile_id": pid, "reason": "profile not found" }));
                    continue;
                };
                if state.manager.is_running(&prof.id).await {
                    skipped.push(json!({ "profile": prof.name, "reason": "already running" }));
                    continue;
                }
                let Some(acc_id) = prof.account_id.as_deref() else {
                    skipped.push(json!({ "profile": prof.name, "reason": "no account assigned" }));
                    continue;
                };
                let Some(acc) = accounts.get(acc_id).cloned() else {
                    skipped.push(json!({ "profile": prof.name, "reason": "assigned account missing" }));
                    continue;
                };
                let acc = refresh_if_needed(&settings, &mut accounts, acc).await?;
                items.push((prof.clone(), acc));
            }
            state.save_accounts(&accounts)?;

            if let Some(c) = cols.get_mut(&col_id) {
                c.last_started_at = Some(chrono::Utc::now());
            }
            state.save_collections(&cols)?;

            let launch_results = state
                .manager
                .launch_batch_with_stagger(&state.paths, &settings, items, col.stagger_ms)
                .await;
            let formatted: Vec<Value> = launch_results
                .into_iter()
                .map(|(id, r)| {
                    let pname = profiles
                        .get(&id)
                        .map(|p| p.name.as_str())
                        .unwrap_or(&id);
                    match r {
                        Ok(()) => json!({ "profile": pname, "status": "started" }),
                        Err(e) => json!({ "profile": pname, "status": "failed", "error": e.to_string() }),
                    }
                })
                .collect();
            Ok(json!({ "collection": col.name, "launched": formatted, "skipped": skipped }))
        }
        "launch_profiles" => {
            let refs = args
                .get("profiles")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing 'profiles' array"))?;
            let profiles = state.load_profiles()?;
            let mut accounts = state.load_accounts()?;
            let settings = state.load_settings()?;

            let mut items = Vec::new();
            for r in refs {
                let s = r
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Profile entry must be a string"))?;
                let prof = resolve_profile(&profiles, s)?.clone();
                let acc_id = prof
                    .account_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Profile '{}' has no account assigned", prof.name))?;
                let acc = accounts
                    .get(acc_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Assigned account missing for '{}'", prof.name))?;
                let acc = refresh_if_needed(&settings, &mut accounts, acc).await?;
                items.push((prof, acc));
            }
            state.save_accounts(&accounts)?;

            let results = state
                .manager
                .launch_batch(&state.paths, &settings, items)
                .await;
            let formatted: Vec<Value> = results
                .into_iter()
                .map(|(id, r)| match r {
                    Ok(()) => json!({ "profile_id": id, "status": "started" }),
                    Err(e) => json!({ "profile_id": id, "status": "failed", "error": e.to_string() }),
                })
                .collect();
            Ok(json!({ "results": formatted }))
        }
        "stop_instance" => {
            let target = req_str(&args, "profile")?;
            if target.eq_ignore_ascii_case("all") {
                state.manager.stop_all().await?;
                Ok(json!({ "stopped": "all" }))
            } else {
                let profiles = state.load_profiles()?;
                let p = resolve_profile(&profiles, &target)?;
                state.manager.stop_profile(&p.id).await?;
                Ok(json!({ "stopped": p.name }))
            }
        }
        "set_window_hidden" => {
            let target = req_str(&args, "profile")?;
            let hidden = args
                .get("hidden")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| anyhow::anyhow!("'hidden' (boolean) is required"))?;
            let profiles = state.load_profiles()?;
            let p = resolve_profile(&profiles, &target)?;
            let paths = mub_core::AppPaths::default_root()?;
            let rec = mub_core::RunningStore::alive(&paths.running_file())
                .into_iter()
                .find(|r| r.profile_id == p.id)
                .ok_or_else(|| anyhow::anyhow!("'{}' is not running", p.name))?;
            // Keep this session's keeper in sync, then act on the window directly (works for
            // instances started by the GUI or another mub process too).
            state.manager.set_window_hidden(&p.id, hidden, !hidden).await;
            let n = tokio::task::spawn_blocking(move || {
                if hidden {
                    mub_core::window::hide_pid(rec.pid)
                } else {
                    mub_core::window::show_pid(rec.pid, true)
                }
            })
            .await
            .unwrap_or(0);
            Ok(json!({ "profile": p.name, "hidden": hidden, "windows": n }))
        }
        "list_running_instances" => {
            let list = state.manager.list_statuses().await;
            let items: Vec<Value> = list
                .into_iter()
                .map(|s| {
                    let elapsed = s.elapsed_secs();
                    json!({
                        "profile_id": s.profile_id,
                        "profile_name": s.profile_name,
                        "account_name": s.account_name,
                        "pid": s.pid,
                        "state": format!("{:?}", s.state),
                        "progress": s.progress,
                        "elapsed_secs": elapsed,
                    })
                })
                .collect();
            Ok(json!({ "instances": items }))
        }
        "get_instance_logs" => {
            let target = req_str(&args, "profile")?;
            let tail = args.get("tail").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
            let profiles = state.load_profiles()?;
            let p = resolve_profile(&profiles, &target)?;
            let status = state
                .manager
                .get_status(&p.id)
                .await
                .ok_or_else(|| anyhow::anyhow!("No tracked session for '{}'", p.name))?;
            let total = status.logs.len();
            let start = total.saturating_sub(tail);
            let lines: Vec<String> = status.logs.iter().skip(start).cloned().collect();
            Ok(json!({ "profile": p.name, "lines": lines }))
        }
        other => anyhow::bail!("Unknown MCP tool '{other}'"),
    }
}

async fn batch_create_bots(state: &McpState, args: Value) -> Result<Value> {
    let prefix = req_str(&args, "name_prefix")?;
    let explicit_accounts: Vec<String> = args
        .get("accounts")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let auto_assign = args
        .get("auto_assign_bot_accounts")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let prepare = args
        .get("prepare")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut profiles = state.load_profiles()?;
    let accounts = state.load_accounts()?;
    let settings = state.load_settings()?;

    let template = if let Some(tpl_ref) = args.get("template_profile").and_then(|v| v.as_str()) {
        Some(resolve_profile(&profiles, tpl_ref)?.clone())
    } else {
        None
    };

    let base_mc_loader = if let Some(ref tpl) = template {
        (tpl.mc_version.clone(), tpl.mod_loader.clone())
    } else {
        let mc = req_str(&args, "mc_version")?;
        let loader_name = args
            .get("loader")
            .and_then(|v| v.as_str())
            .unwrap_or("vanilla");
        let loader_ver = args.get("loader_version").and_then(|v| v.as_str());
        let ml = resolve_mod_loader(loader_name, &mc, loader_ver).await?;
        (mc, ml)
    };

    // Determine target account IDs
    let assigned_ids: Vec<String> = if !explicit_accounts.is_empty() {
        explicit_accounts
            .iter()
            .map(|r| resolve_account(&accounts, r).map(|a| a.id.clone()))
            .collect::<Result<Vec<_>>>()?
    } else if auto_assign {
        let used_ids: std::collections::HashSet<&str> = profiles
            .profiles
            .iter()
            .filter_map(|p| p.account_id.as_deref())
            .collect();
        accounts
            .accounts
            .iter()
            .filter(|a| a.role == AccountRole::Bot && !used_ids.contains(a.id.as_str()))
            .map(|a| a.id.clone())
            .collect()
    } else {
        Vec::new()
    };

    let count = args
        .get("count")
        .and_then(|v| v.as_u64())
        .map(|c| c as usize)
        .unwrap_or_else(|| {
            if !explicit_accounts.is_empty() {
                explicit_accounts.len()
            } else {
                assigned_ids.len()
            }
        });

    if count == 0 {
        anyhow::bail!(
            "count resolved to 0 (specify 'count', 'accounts', or have unassigned bot accounts)"
        );
    }

    let quick_connect = args
        .get("quick_connect")
        .and_then(|v| v.as_str())
        .map(String::from);
    let mem_max = args.get("memory_max_mb").and_then(|v| v.as_u64()).map(|m| m as u32);

    let mut created = Vec::new();
    let mut created_ids = Vec::new();
    let mut idx = 1usize;

    while created.len() < count {
        let candidate_name = format!("{prefix}-{idx}");
        idx += 1;
        if profiles.find_by_name(&candidate_name).is_some() {
            continue;
        }
        let mut prof = if let Some(ref tpl) = template {
            let mut p = tpl.clone_with_name(&candidate_name);
            p.kind = ProfileKind::Bot;
            p
        } else {
            Profile::new(
                &candidate_name,
                ProfileKind::Bot,
                &base_mc_loader.0,
                base_mc_loader.1.clone(),
            )
        };

        if let Some(acc_id) = assigned_ids.get(created.len()) {
            prof.account_id = Some(acc_id.clone());
        }
        if let Some(ref qc) = quick_connect {
            prof.quick_connect = Some(qc.clone());
        }
        if let Some(max) = mem_max {
            prof.memory_max_mb = max;
        }

        std::fs::create_dir_all(state.paths.instance_dir(&prof.id))?;
        if let Some(ref tpl) = template {
            copy_profile_content(&state.paths, &tpl.id, &prof.id)?;
        }

        created_ids.push(prof.id.clone());
        created.push(prof);
    }

    if prepare && let Some(first) = created.first() {
        let java_bin = resolve_java_async(&settings, first).await;
        prepare_profile_version(&state.paths, &settings, first, &java_bin, |_| {}).await?;
    }

    let mut summaries = Vec::with_capacity(created.len());
    for p in created {
        summaries.push(profile_summary(&state.paths, &p, &accounts));
        profiles.upsert(p);
    }
    state.save_profiles(&profiles)?;

    let mut collection_info = Value::Null;
    if let Some(col_name) = args.get("collection_name").and_then(|v| v.as_str()) {
        let mut cols = state.load_collections()?;
        let col = if let Some(existing) = cols.find_by_name_mut(col_name) {
            for pid in &created_ids {
                existing.add_profile(pid);
            }
            existing.clone()
        } else {
            let mut c = BotCollection::new(col_name);
            for pid in &created_ids {
                c.add_profile(pid);
            }
            cols.upsert(c.clone());
            c
        };
        state.save_collections(&cols)?;
        collection_info = json!({ "id": col.id, "name": col.name, "member_count": col.profile_ids.len() });
    }

    Ok(json!({
        "created_count": summaries.len(),
        "prepared": prepare,
        "collection": collection_info,
        "bots": summaries,
    }))
}

fn req_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing required string parameter '{key}'"))
}

fn parse_role(s: &str) -> Result<AccountRole> {
    match s.to_lowercase().as_str() {
        "player" => Ok(AccountRole::Player),
        "bot" => Ok(AccountRole::Bot),
        _ => anyhow::bail!("Invalid role '{s}'. Expected 'player' or 'bot'."),
    }
}

fn parse_kind(s: &str) -> Result<ProfileKind> {
    match s.to_lowercase().as_str() {
        "player" => Ok(ProfileKind::Player),
        "bot" => Ok(ProfileKind::Bot),
        _ => anyhow::bail!("Invalid kind '{s}'. Expected 'player' or 'bot'."),
    }
}

fn parse_content_kind(s: &str) -> Result<ContentKind> {
    match s.to_lowercase().as_str() {
        "mod" => Ok(ContentKind::Mod),
        "modpack" => Ok(ContentKind::Modpack),
        "resourcepack" => Ok(ContentKind::ResourcePack),
        "shader" => Ok(ContentKind::Shader),
        "datapack" => Ok(ContentKind::DataPack),
        _ => anyhow::bail!("Invalid content kind '{s}'."),
    }
}

fn parse_source_filter(s: &str) -> Result<SourceFilter> {
    match s.to_lowercase().as_str() {
        "all" => Ok(SourceFilter::All),
        "modrinth" => Ok(SourceFilter::Modrinth),
        "curseforge" => Ok(SourceFilter::CurseForge),
        "github" | "gh" => Ok(SourceFilter::GitHub),
        _ => anyhow::bail!("Invalid source '{s}'."),
    }
}

fn resolve_account<'a>(store: &'a AccountStore, id_or_name: &str) -> Result<&'a Account> {
    store
        .get(id_or_name)
        .or_else(|| store.find_by_username(id_or_name))
        .ok_or_else(|| anyhow::anyhow!("Account '{id_or_name}' not found"))
}

fn resolve_profile<'a>(store: &'a ProfileStore, id_or_name: &str) -> Result<&'a Profile> {
    store
        .get(id_or_name)
        .or_else(|| store.find_by_name(id_or_name))
        .ok_or_else(|| anyhow::anyhow!("Profile '{id_or_name}' not found"))
}

async fn resolve_mod_loader(
    loader: &str,
    mc_version: &str,
    loader_version: Option<&str>,
) -> Result<ModLoader> {
    let vm = VersionManager::new();
    match loader.to_lowercase().as_str() {
        "vanilla" => Ok(ModLoader::Vanilla),
        "fabric" => {
            let ver = match loader_version {
                Some(v) => v.to_string(),
                None => vm
                    .fetch_fabric_loader_versions(mc_version)
                    .await?
                    .into_iter()
                    .find(|v| v.stable)
                    .map(|v| v.version)
                    .ok_or_else(|| anyhow::anyhow!("No Fabric loader found for {mc_version}"))?,
            };
            Ok(ModLoader::Fabric {
                loader_version: ver,
            })
        }
        "quilt" => {
            let ver = match loader_version {
                Some(v) => v.to_string(),
                None => vm
                    .fetch_quilt_loaders(mc_version)
                    .await?
                    .into_iter()
                    .next()
                    .map(|v| v.version)
                    .ok_or_else(|| anyhow::anyhow!("No Quilt loader found for {mc_version}"))?,
            };
            Ok(ModLoader::Quilt {
                loader_version: ver,
            })
        }
        "neoforge" => {
            let ver = match loader_version {
                Some(v) => v.to_string(),
                None => vm
                    .fetch_neoforge_versions(mc_version)
                    .await?
                    .into_iter()
                    .next()
                    .map(|v| v.version)
                    .ok_or_else(|| anyhow::anyhow!("No NeoForge loader found for {mc_version}"))?,
            };
            Ok(ModLoader::NeoForge {
                loader_version: ver,
            })
        }
        "forge" => {
            let ver = loader_version
                .ok_or_else(|| anyhow::anyhow!("loader_version is required for Forge"))?;
            Ok(ModLoader::Forge {
                loader_version: ver.to_string(),
            })
        }
        other => anyhow::bail!("Unknown mod loader '{other}'"),
    }
}

async fn resolve_java_async(settings: &LauncherSettings, profile: &Profile) -> String {
    let prof_java = profile.java_path.clone();
    let def_java = settings.default_java_path.clone();
    let mc_ver = profile.mc_version.clone();
    tokio::task::spawn_blocking(move || {
        resolve_java_for_mc(prof_java.as_deref(), &def_java, &mc_ver)
    })
    .await
    .unwrap_or_else(|_| {
        profile
            .java_path
            .clone()
            .unwrap_or_else(|| settings.default_java_path.clone())
    })
}

async fn refresh_if_needed(
    settings: &LauncherSettings,
    accounts: &mut AccountStore,
    acc: Account,
) -> Result<Account> {
    if acc.is_msa() && !acc.is_token_valid() {
        let client = MsaAuthClient::new(&settings.msa_client_id);
        let refreshed = client.refresh_account(&acc).await?;
        accounts.upsert(refreshed.clone());
        Ok(refreshed)
    } else {
        Ok(acc)
    }
}

fn copy_profile_content(paths: &AppPaths, src_id: &str, dst_id: &str) -> Result<()> {
    for (src_dir, dst_dir) in [
        (paths.mods_dir(src_id), paths.mods_dir(dst_id)),
        (paths.config_dir(src_id), paths.config_dir(dst_id)),
        (
            paths.resourcepacks_dir(src_id),
            paths.resourcepacks_dir(dst_id),
        ),
        (paths.shaderpacks_dir(src_id), paths.shaderpacks_dir(dst_id)),
    ] {
        if src_dir.exists() {
            copy_dir_recursive(&src_dir, &dst_dir)?;
        }
    }
    let src_manifest = mub_core::catalog::content_manifest_path(paths, src_id);
    if src_manifest.exists() {
        let dst_manifest = mub_core::catalog::content_manifest_path(paths, dst_id);
        if let Some(parent) = dst_manifest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src_manifest, dst_manifest)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn profile_summary(paths: &AppPaths, p: &Profile, accounts: &AccountStore) -> Value {
    let acc_name = p
        .account_id
        .as_deref()
        .and_then(|id| accounts.get(id))
        .map(|a| a.username.as_str());
    let mods_count = list_installed_content(paths, &p.id, ContentKind::Mod)
        .map(|v| v.len())
        .unwrap_or(0);
    json!({
        "id": p.id,
        "name": p.name,
        "kind": p.kind,
        "mc_version": p.mc_version,
        "mod_loader": p.mod_loader.to_string(),
        "account_id": p.account_id,
        "account_username": acc_name,
        "group": p.group,
        "quick_connect": p.quick_connect,
        "memory_min_mb": p.memory_min_mb,
        "memory_max_mb": p.memory_max_mb,
        "mods_count": mods_count,
    })
}

fn collection_summary(
    c: &BotCollection,
    profiles: &ProfileStore,
    accounts: &AccountStore,
    statuses: &[mub_core::InstanceStatus],
) -> Value {
    let members: Vec<Value> = c
        .profile_ids
        .iter()
        .map(|pid| {
            if let Some(p) = profiles.get(pid) {
                let acc = p
                    .account_id
                    .as_deref()
                    .and_then(|id| accounts.get(id))
                    .map(|a| a.username.as_str());
                let state = statuses
                    .iter()
                    .find(|s| s.profile_id == p.id)
                    .map(|s| format!("{:?}", s.state))
                    .unwrap_or_else(|| "Idle".to_string());
                json!({
                    "id": p.id,
                    "name": p.name,
                    "account": acc,
                    "mc_version": p.mc_version,
                    "state": state,
                })
            } else {
                json!({ "id": pid, "missing": true })
            }
        })
        .collect();
    json!({
        "id": c.id,
        "name": c.name,
        "description": c.description,
        "stagger_ms": c.stagger_ms,
        "member_count": c.profile_ids.len(),
        "last_started_at": c.last_started_at,
        "members": members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_initialize_tools_and_batch_bots_flow() {
        let dir = std::env::temp_dir().join(format!("mub_mcp_test_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&dir);
        paths.ensure_dirs().unwrap();
        let state = McpState::new(paths);

        // 1. initialize
        let init_resp = handle_request(
            &state,
            JsonRpcRequest {
                jsonrpc: Some("2.0".into()),
                id: Some(json!(1)),
                method: "initialize".into(),
                params: json!({}),
            },
        )
        .await
        .unwrap();
        assert!(init_resp.error.is_none());
        assert_eq!(
            init_resp.result.unwrap()["serverInfo"]["name"],
            "modsundbots-mcp"
        );

        // 2. tools/list
        let list_resp = handle_request(
            &state,
            JsonRpcRequest {
                jsonrpc: Some("2.0".into()),
                id: Some(json!(2)),
                method: "tools/list".into(),
                params: json!({}),
            },
        )
        .await
        .unwrap();
        let tools = list_resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert!(tools.iter().any(|t| t["name"] == "batch_create_bots"));
        assert!(tools.iter().any(|t| t["name"] == "start_bot_collection"));

        // 3. Add two offline bot accounts
        for bot_name in ["AlphaBot", "BetaBot"] {
            let r = call_tool(
                &state,
                "add_offline_account",
                json!({ "username": bot_name, "role": "bot" }),
            )
            .await
            .unwrap();
            assert_eq!(r["username"], bot_name);
        }

        // 4. Batch create 2 bots into a collection
        let batch = call_tool(
            &state,
            "batch_create_bots",
            json!({
                "name_prefix": "Miner",
                "count": 2,
                "mc_version": "1.21.4",
                "loader": "vanilla",
                "quick_connect": "play.example.com:25565",
                "collection_name": "MiningSquad"
            }),
        )
        .await
        .unwrap();
        assert_eq!(batch["created_count"], 2);
        assert_eq!(batch["collection"]["name"], "MiningSquad");
        assert_eq!(batch["collection"]["member_count"], 2);
        assert_eq!(batch["bots"][0]["account_username"], "AlphaBot");
        assert_eq!(batch["bots"][1]["account_username"], "BetaBot");

        // 5. List collections
        let cols = call_tool(&state, "list_bot_collections", json!({}))
            .await
            .unwrap();
        assert_eq!(cols["collections"].as_array().unwrap().len(), 1);
        assert_eq!(cols["collections"][0]["members"].as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
