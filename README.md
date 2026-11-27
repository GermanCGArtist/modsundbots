# ModsUndBots

A fast, low-overhead Minecraft launcher written in Rust for running many instances at once, for
**player** and **bot** accounts. Versions, libraries and assets are shared between all instances, so
dozens of bot profiles cost little disk space. Use it through the GUI, the `mub` CLI, or an MCP server
for AI agents.

- [Features](#features)
- [Install](#install)
- [Getting started](#getting-started)
- [The GUI](#the-gui)
- [Bot Wall: hidden bot windows](#bot-wall-hidden-bot-windows)
- [CLI reference](#cli-reference)
- [MCP server](#mcp-server)
- [Data folder and settings](#data-folder-and-settings)
- [Java selection](#java-selection)
- [Building and releases](#building-and-releases)

## Features

- **Players and bots.** Every profile is a Player or a Bot. Bots get smaller defaults (256–1024 MB heap,
  640×360 window, hidden window) and can be started in groups.
- **Accounts.** Microsoft accounts (browser login or device code) and offline accounts for bots.
- **Mod loaders.** Vanilla, Fabric, Quilt and NeoForge are installed automatically. Forge must be
  installed by hand.
- **Content.** Search and install mods, modpacks, resource packs, shaders and data packs from Modrinth,
  CurseForge and GitHub releases, with dependencies. Check for updates and sync content to all bots.
- **Modpacks.** Import `.mrpack` and CurseForge `.zip`, export a profile as `.mrpack`.
- **Bot collections.** Named groups of profiles with "Start all" and an optional delay between launches.
- **Bot Wall.** Bot windows stay off the desktop; watch them as live thumbnails and click one to play.
- **Re-attach.** Games keep running when the GUI closes; a restarted GUI picks them up again.
- **Self-update** from GitHub Releases.

## Install

Download the archive for your system from the
[Releases](https://github.com/GermanCGArtist/modsundbots/releases) page (Windows x86_64, macOS Apple
Silicon / Intel as `ModsUndBots.app`, Linux x86_64). It contains the GUI (`modsundbots`) and the CLI
(`mub`). Later updates install from the GUI (*Config → Updates*) or with `mub update`.

To build it yourself, see [Building and releases](#building-and-releases).

## Getting started

In the GUI:

1. **Accounts:** add a Microsoft account (players) or an offline name (bots).
2. **Instances → New profile:** pick Player or Bot, Minecraft version, loader, account and an optional
   server to join on start.
3. Add mods under **Discover** or the profile's *Content* tab, or drop a `.mrpack`, `.zip` or `.jar`
   onto the window.
4. Start the profile with ▶ in *Quick launch* (or from the Instances list). Bots start hidden and show up
   on the **WALL** tab.

The same with the CLI:

```bash
mub account add-offline Bot01 --role bot
```

```bash
mub profile create bot01 --kind bot --mc-version 26.3 --fabric 0.17.2 --account Bot01 --server play.example.net
```

```bash
mub mods install fabric-api --profile bot01
```

```bash
mub launch bot01
```

## The GUI

The sidebar has seven views; **Ctrl+K** opens a command palette for all of them.

| View | What it does |
|---|---|
| 01 Instances | All profiles with filters (all / players / bots), search, multi-select, launch and stop. Open a profile for its *Content*, *Worlds & files*, *Logs* and *Settings* (version, loader, account, memory, Java, JVM args, resolution, quick-connect server, hide window). |
| 02 Collections | Bot collections: create, edit, prepare (download everything) and start or stop all. |
| 03 Discover | Browse Modrinth, CurseForge and GitHub; install into one profile or all bots. |
| 04 Accounts | Microsoft and offline accounts; refresh tokens. |
| 05 Console | Live log of each instance with level filter (ALL / INFO / WARN / ERR), search and copy. |
| 06 Wall | Live thumbnails of all running instances, see [Bot Wall](#bot-wall-hidden-bot-windows). |
| 07 Config | Theme, CurseForge key, default Java (with detection), launch delay for batches, parallel downloads, Microsoft client ID, MCP setup, app updates (with GitHub token). |

Four themes are built in: Synth Terminal (default), Gruvbox Pixel, Tokyo Night and Drafting Paper.
Dropping files onto the window imports `.mrpack` / `.zip` as a new profile and installs a `.jar` into the
selected profile's `mods/`.

## Bot Wall: hidden bot windows

Bot profiles start with their game window hidden, so many bots don't clutter the desktop. Open the
**WALL** tab to watch them as live thumbnails (about one picture per second, bots first).

- Click a picture (or **SHOW**) to bring that game to the front and play; **HIDE** sends it back.
- **HIDE ALL BOTS** and **SHOW ALL** act on every running instance. **UNHIDE LOST** shows every hidden
  Java game window on the PC, also ones mub no longer tracks.
- **LOGS** opens the instance in the Console.
- Per profile: *Settings → HIDE WINDOW*. Default: on for bots, off for players.

How it works:

- A hidden window is parked off-screen, without a taskbar button or Alt-Tab entry. It is not minimized,
  so the game keeps running and the Wall can still capture it.
- When a game starts, its window can show for a moment (about 30 ms from the GUI, up to about a second
  while the game loads) before it is parked. It never takes the keyboard focus.
- Hidden games still render, but Minecraft's own AFK limiter (`inactivityFpsLimit: afk`, which mub keeps
  set for hidden profiles) drops them to 30 fps after 1 minute and 10 fps after 10 minutes without real
  keyboard or mouse input. Input after you click SHOW brings full speed back at once.
- Every game mub starts (GUI, CLI or MCP) is recorded in `running.json` with its process ID. A restarted
  GUI re-attaches to games that still run: status, Wall, log (read from `logs/latest.log`) and Stop all
  work again. Games started by the CLI or the MCP server also appear on the Wall.
- Only games started by mub are on the Wall; games from other launchers are not touched.

If the GUI crashed while bots were hidden, bring all windows back with:

```bash
mub windows unhide --all
```

## CLI reference

`mub <command> --help` shows every option. Profiles and accounts can be given by name or ID.

| Command | What it does |
|---|---|
| `mub account list` / `add-msa` / `add-offline <name> [--role bot\|player]` / `refresh` / `remove` | Manage accounts |
| `mub profile list` / `create <name>` / `clone` / `remove` | Manage profiles. `create` takes `--kind`, `--mc-version`, `--fabric/--quilt/--neoforge/--forge <loader version>`, `--group`, `--account`, `--server` |
| `mub launch <profile>...` | Start profiles and wait until they exit; prints exit codes and the last log lines of failed games |
| `mub launch-all bot\|player` | Start all profiles of one kind |
| `mub mods search <query>` | Search content; `--source all\|modrinth\|curseforge\|github`, `--kind mod\|modpack\|resourcepack\|shader\|datapack`, `--mc-version`, `--fabric`, `--limit` |
| `mub mods install <project>` | Install with dependencies into `--profile <p>` or `--all-bots` |
| `mub mods update <profile>` / `list <profile>` / `remove` | Update, list or remove installed content |
| `mub pack import <file> [--name] [--kind]` | Import a `.mrpack` or CurseForge `.zip` as a new profile |
| `mub pack export <profile> <output> [--version] [--summary]` | Export a profile as `.mrpack` |
| `mub java detect` | List installed Java runtimes |
| `mub collection list` / `create <name> [--profiles ...] [--stagger-ms]` / `add` / `remove` / `delete` / `prepare` / `start` | Bot collections |
| `mub windows list` / `show <profile>` / `hide <profile>` / `unhide --all` | Game windows of running instances |
| `mub mcp` | Run the MCP server over stdio |
| `mub update [--check] [--token <PAT>]` | Check for or install a new version |

## MCP server

`mub mcp` is a Model Context Protocol server (JSON-RPC 2.0 over stdio), so AI clients such as Claude,
Cursor or Antigravity can control ModsUndBots. Changes it makes show up in the GUI right away. *Config →
COPY MCP CONFIG JSON* copies a ready config with the right path:

```json
{
  "mcpServers": {
    "modsundbots": {
      "command": "C:\\path\\to\\mub.exe",
      "args": ["mcp"]
    }
  }
}
```

Tools:

| Area | Tools |
|---|---|
| Overview | `get_status_summary`, `detect_java`, `list_mc_versions`, `list_loader_versions` |
| Accounts | `list_accounts`, `add_offline_account`, `remove_account`, `start_msa_device_login`, `complete_msa_device_login` |
| Profiles | `list_profiles`, `get_profile`, `create_profile`, `clone_profile`, `batch_create_bots`, `update_profile` (incl. `hide_window`), `assign_account`, `delete_profile`, `prepare_profile` |
| Content | `search_mods`, `install_mod`, `list_installed_mods`, `remove_installed_mod`, `import_modpack` |
| Collections | `list_bot_collections`, `create_bot_collection`, `update_bot_collection`, `delete_bot_collection`, `prepare_bot_collection`, `start_bot_collection` |
| Running games | `launch_profiles`, `stop_instance`, `list_running_instances`, `get_instance_logs`, `set_window_hidden` |

## Data folder and settings

| System | Default data folder |
|---|---|
| Windows | `%LOCALAPPDATA%\ModsUndBots\ModsUndBots\data` |
| macOS | `~/Library/Application Support/dev.modsundbots.ModsUndBots` |
| Linux | `~/.local/share/modsundbots` |

Inside it:

| Path | Content |
|---|---|
| `accounts.json`, `profiles.json`, `collections.json` | Your setup (the GUI reloads them when the CLI or MCP server changes them) |
| `settings.json` | Launcher settings (read at start) |
| `running.json` | Games started by mub that may still run (used to re-attach) |
| `shared/` | Versions, libraries and assets, shared by all instances |
| `instances/<profile id>/` | Each instance's game folder (`mods/`, `saves/`, `logs/`, `options.txt`, ...) |

`settings.json` holds the default Java path, launch delay for batches (`batch_launch_stagger_ms`),
parallel downloads, theme, CurseForge API key, GitHub token and extra GitHub mod repos
(`github_mod_repos`, `"owner/repo"` entries searched besides the featured ones). All but the mod repos are
editable under *Config*.

Environment variables:

| Variable | Effect |
|---|---|
| `MUB_HOME` | Use another data folder (separate test setups, portable installs) |
| `MUB_START_VIEW` | `wall` or `console`: open the GUI on that view |
| `MUB_CURSEFORGE_API_KEY` | CurseForge API key (instead of the setting) |
| `MUB_GITHUB_TOKEN` / `GITHUB_TOKEN` | GitHub token for updates and GitHub content |

## Java selection

Each profile can name its own Java runtime (*Settings → Java*). Otherwise mub picks an installed runtime
that fits the Minecraft version: Java 8 up to 1.16, 16 for 1.17, 17 for 1.18–1.20.4, 21 for 1.20.5–1.21.x
and Java 25 for 26.1 and newer. `mub java detect` lists what it finds.

## Building and releases

Needs a recent stable Rust toolchain (edition 2024).

```bash
cargo run -p mub-gui
```

```bash
cargo run -p mub-cli -- --help
```

```bash
cargo build --release
```

```bash
cargo test
```

The workspace has three crates:

- `crates/mub-core`: accounts, profiles, version and loader installation, content catalogs, the instance
  manager, `running.json` and the window helpers (`window.rs`, hide/show/capture on Windows).
- `crates/mub-cli`: the `mub` CLI and the MCP server (`mcp.rs`).
- `crates/mub-gui`: the `modsundbots` GUI (eframe / egui). `examples/wallprobe.rs` is a small tool to
  check window hiding and capture by hand.

Pushing a tag `v*` builds Windows, macOS (both architectures) and Linux binaries and attaches them to a
GitHub release (`.github/workflows/release.yml`).
