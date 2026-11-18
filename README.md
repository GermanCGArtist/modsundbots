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
