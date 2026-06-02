# AGENTS.md

## Project Overview

`piquelcli` is a small Rust 2024 CLI for personal machine/session management. It reads JSON configuration, exposes commands with `clap`, and drives `tmux` sessions/windows from that configuration. Nix files provide the package, development shell, and a NixOS module wrapper that installs the CLI as `piquel`.

Primary source layout:

- `src/main.rs`: binary entry point.
- `src/lib.rs`: shared data types and helpers.
- `src/cli.rs`: command-line parser and top-level dispatch.
- `src/cli/sessions.rs`: CLI command handlers for session workflows.
- `src/config.rs`: JSON config loading and global config access.
- `src/tmux.rs`: tmux command integration.
- `nix/`: package, shell, and NixOS module definitions.

## Local Commands

Use the Nix shell when available:

```sh
nix develop
```

Common checks:

```sh
cargo fmt --check
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
nix flake check
```

For local manual runs, pass an explicit config path unless your machine has the expected default file:

```sh
cargo run -- --config /path/to/config.json list -c
cargo run -- --config /path/to/config.json load <session>
cargo run -- --config /path/to/config.json session /path/to/root
```

The CLI default config path is `~/.config/piquel/config.json`.

## Development Conventions

- Keep changes small and direct. This is a compact utility; avoid introducing broad abstractions unless they remove real duplication or match an existing boundary.
- Prefer idiomatic Rust error handling with `Result` and concrete error enums where module-local errors are useful.
- Keep the command surface in `src/cli.rs`; put command behavior in `src/cli/sessions.rs` or a focused sibling module.
- Keep tmux process execution details inside `src/tmux.rs`.
- Keep config parsing, validation, and global config access inside `src/config.rs` and the config data types in `src/lib.rs`.
- Run `cargo fmt` before finishing Rust edits.

## Config Model

The JSON config maps to:

- `Config`
  - `sessions`: map of session name to `SessionConfig`
  - `validate_session_root`: whether configured roots must exist and be directories
  - `default_session`: window definitions used by the ad hoc `session` command
- `SessionConfig`
  - `root`: session root path
  - `windows`: list of windows
- `WindowConfig`
  - `commands`: commands sent to tmux with `send-keys`

Validation currently rejects empty session names and names containing `.`, expands `~`, optionally validates roots, and requires at least one window.

## Tmux Behavior

`load` and `session` intentionally fail when already inside tmux through `tmux::err_in_tmux()`. Preserve that behavior unless the user explicitly requests a behavior change.

Be careful with tmux commands in tests and manual verification:

- `list` can be checked without creating a session.
- `load` and `session` may attach the terminal to tmux.
- Prefer unit tests for parsing/validation logic and keep tmux process integration behind functions that can be reasoned about separately.

## Nix Notes

- `nix/pkg.nix` builds the Rust package from `Cargo.toml` and `Cargo.lock`.
- `nix/shell.nix` provides Rust tooling.
- `nix/module.nix` exposes `programs.piquelcli` and wraps the binary as `piquel` with `--config <generated-json>`.

When changing CLI flags, config schema, or binary names, update the Nix module and package wrapper as needed.

## Git Hygiene

- Do not revert unrelated local changes.
- Keep `Cargo.lock` in sync when dependencies change.
- Do not modify generated build output under `target/`.
