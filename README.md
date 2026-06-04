# piquel-cli

`piquel` is a small Rust CLI for opening project and ad hoc workspaces in
tmux. It reads a JSON config, lists configured projects, opens tmux sessions
from reusable templates, and can route through `fzf` when picking a running
session or project interactively.

## Requirements

- Rust 1.91 or newer, as pinned by `rust-toolchain.toml`
- `tmux` for session commands
- `fzf` for `piquel pick`
- `git` when using project worktree selection

## Install

Build locally:

```sh
cargo build --release
```

Then put `target/release/piquel` somewhere on your `PATH`.

With the included Nix flake:

```sh
nix build
```

## Usage

By default, the CLI reads `~/.config/piquel/config.json`. Use `--config` to
point at a different file.

```sh
piquel --config examples/test-config.json project list
```

Commands:

```text
piquel list
piquel pick
piquel project list
piquel project load <project> [--session <template>] [--worktree <branch>]
piquel session [path] [--session <template>] [--name <tmux-name>]
```

`piquel list` prints running tmux sessions. `piquel pick` combines running
tmux sessions and configured projects, lets `fzf` select one, then attaches to
the session or opens the project.

`piquel project load` opens a configured project. When `--worktree` is set,
the CLI runs `git worktree list --porcelain` under the project path and opens
the matching branch worktree.

`piquel session` opens an arbitrary directory with a configured session
template. If no path is given, it uses the current working directory. If no
name is given, it derives the tmux session name from the directory name.

## Configuration

Minimal config:

```json
{
  "default_session": "default",
  "sessions": {
    "default": {
      "windows": [
        {
          "commands": []
        }
      ]
    }
  }
}
```

Fuller project config:

```json
{
  "projects_dir": "~/Projects",
  "default_session": "default",
  "sessions": {
    "default": {
      "windows": [
        {
          "commands": ["vim ."]
        },
        {
          "commands": ["git status"]
        }
      ]
    },
    "rust": {
      "windows": [
        {
          "commands": ["vim ."]
        },
        {
          "commands": ["cargo test"]
        }
      ]
    }
  },
  "projects": [
    {
      "repository": "git@github.com:PiquelChips/piquel-cli.git",
      "default_session": "rust"
    },
    {
      "repository": "https://github.com/example/custom-name.git",
      "name": "custom",
      "path": "~/src/custom",
      "default_session": {
        "windows": [
          {
            "commands": ["vim ."]
          }
        ]
      }
    }
  ]
}
```

Fields:

- `projects_dir`: base directory for projects without an explicit `path`.
  Defaults to `~/Projects`.
- `default_session`: the session template used when a project does not specify
  one. Defaults to `default`.
- `sessions`: named tmux session templates. Each template must have at least
  one window.
- `projects`: configured repositories. The project name is derived from the
  repository basename unless `name` is set.
- `projects[].path`: explicit project path. Defaults to
  `<projects_dir>/<project-name>`.
- `projects[].default_session`: either the name of a global session template or
  an inline session template.
- `sessions.*.windows[].commands`: commands sent to the created tmux window,
  each followed by `Enter`.

Project and session names must not be empty or contain `:`. tmux session names
derived from worktree branches are sanitized before tmux is invoked.

## Testing

Run the whole test suite:

```sh
cargo test
```

Run linting at the same strictness used during development:

```sh
cargo clippy --all-targets --all-features -- -D warnings
```

Build API docs:

```sh
cargo doc --no-deps --all-features
```

The suite has two layers:

- Unit tests live next to the modules they cover. They exercise config
  validation, project name/path resolution, git worktree parsing, picker target
  construction, fzf cancellation handling, and tmux session-name sanitization.
- Integration tests live in `tests/cli.rs`. They execute the compiled
  `piquel` binary with temporary config files and fake `tmux`, `fzf`, and
  `git` binaries injected into `PATH`. This verifies real CLI behavior without
  depending on a live tmux server or local git worktree setup.
