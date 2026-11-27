# ModsUndBots

A high-performance, low-overhead Minecraft launcher built in Rust, designed for managing and running many Minecraft instances simultaneously for both **player** and **bot** accounts.

## Workspace Structure

- `crates/mub-core` — Core engine: multi-account MSA authentication, shared asset/library/version cache, profile management, and concurrent JVM process manager.
- `crates/mub-cli` — CLI (`mub`) for scripted/headless account, profile, and batch instance management.
- `crates/mub-gui` — Native Rust GUI (`eframe` / `egui`) for low-overhead multi-instance monitoring and batch control.

## Quick Start

```bash
# Run the GUI
cargo run -p mub-gui

# Or use the CLI
cargo run -p mub-cli -- --help
```

## Bot Wall: hidden bot windows

Bot profiles start with their game window hidden, so many bots don't clutter the desktop. Open the
**WALL** tab in the GUI to watch them as live thumbnails (about one picture per second).

- Click a picture (or **SHOW**) to bring that game to the front and play; **HIDE** sends it back.
- **HIDE ALL BOTS** / **SHOW ALL** act on every running instance. **UNHIDE LOST** shows every hidden
  Java game window on the PC, even ones mub no longer tracks.
- Per profile: *Settings → HIDE WINDOW*. Default: on for bots, off for players.
- Hidden windows are parked off-screen without a taskbar button or Alt-Tab entry. They keep rendering,
  but Minecraft's own AFK limiter (`inactivityFpsLimit: afk`, which mub enforces for hidden profiles)
  drops them to 30 fps after 1 minute and 10 fps after 10 minutes without real input.
- Games started by mub (GUI, CLI or MCP) are recorded in `running.json`. A restarted GUI re-attaches
  to them (status, Wall, logs from `logs/latest.log`, Stop).
- If the GUI crashed while bots were hidden:

```bash
mub windows unhide --all
```

Other commands: `mub windows list`, `mub windows show <profile>`, `mub windows hide <profile>`. MCP:
`set_window_hidden`, and `hide_window` on `update_profile`.

`MUB_HOME=<dir>` uses another data folder (separate test setups); `MUB_START_VIEW=wall` opens the GUI on
the Wall.
