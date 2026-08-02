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

By default, the CLI reads `~/.config/piquel/config.json`. Set `PIQUEL_CONFIG`
or use `--config` to point at a different file. The `--config` flag takes
precedence over `PIQUEL_CONFIG`.

```sh
piquel --config examples/test-config.json project list
```

Commands:

```text
piquel completions <shell>
piquel list
piquel pick [project] [--session <template>]
piquel project list
piquel project load <project> [--session <template>] [--worktree <branch>]
piquel session [path] [--session <template>] [--name <tmux-name>]
```

`piquel completions <shell>` prints shell completions for `bash`, `fish`, or
`zsh`. Nix installs these completions automatically. For manual installs,
write the generated completion to the location your shell reads:

```sh
piquel completions zsh > ~/.zfunc/_piquel
piquel completions bash > ~/.local/share/bash-completion/completions/piquel
piquel completions fish > ~/.config/fish/completions/piquel.fish
```

`piquel list` prints running tmux sessions. `piquel pick` combines running
tmux sessions and configured projects, lets `fzf` select one, then attaches to
the session or opens the project branch workflow. `piquel pick <project>` skips
the first picker and opens that project's branch workflow directly.

All commands operate on the local machine. The config file may still contain
`machines` entries, but they are only parsed and validated as configuration
data; command dispatch does not use them.

The branch picker lists local git branches only. It does not fetch and does not
create remote-tracking branches. If the selected branch already has a worktree,
`piquel` opens it. If not, `piquel` creates a managed worktree at
`<worktrees_dir>/<project>/<sanitized-branch>`.

Branch-selected tmux sessions use `project--branch` names, with the branch
sanitized for tmux. This also applies when the selected branch is checked out at
the configured project path. Branch-aware workflows use the plain `project`
session name only when the project has no local branches.

`piquel pick --session <template>` uses the template override only when a
project is opened. Selecting an existing tmux session still attaches to that
session unchanged.

`piquel project load` opens a configured project. When `--worktree` is set,
the CLI opens the requested local branch worktree, creating a managed worktree
if the branch does not already have one. Bare `piquel project load <project>`
opens the configured project path with tmux session name `project`.

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
  "worktrees_dir": "~/.piquel/worktrees",
  "default_session": "default",
  "sessions": {
    "default": {
      "windows": [
        {
          "name": "editor",
          "commands": ["vim ."]
        },
        {
          "name": "git",
          "commands": ["git status"]
        }
      ]
    },
    "rust": {
      "windows": [
        {
          "name": "editor",
          "commands": ["vim ."]
        },
        {
          "name": "test",
          "commands": ["cargo test"]
        }
      ]
    }
  },
  "machines": [
    {
      "name": "pi",
      "address": "pi.local",
      "username": "ronan"
    }
  ],
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
            "name": "editor",
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
- `worktrees_dir`: base directory for managed branch worktrees. Defaults to
  `~/.piquel/worktrees`.
- `default_session`: the session template used when a project does not specify
  one. Defaults to `default`.
- `sessions`: named tmux session templates. Each template must have at least
  one window.
- `projects`: configured repositories. The project name is derived from the
  repository basename unless `name` is set.
- `machines`: configured machine records. They are validated and preserved in
  the schema, but commands do not currently use them.
- `projects[].path`: explicit project path. Defaults to
  `<projects_dir>/<project-name>`.
- `projects[].default_session`: either the name of a global session template or
  an inline session template.
- `sessions.*.windows[].name`: optional tmux window name.
- `sessions.*.windows[].commands`: commands sent to the created tmux window,
  each followed by `Enter`.

Project names must be safe path segments: they must not be empty, `.`, `..`, or
contain `/`, `\`, or `:`. Session names must not be empty or contain `:`. tmux
session names derived from worktree branches are sanitized before tmux is
invoked. Window names, when set, must not be blank.

## Command Model

Commands are local process executions. Modules that own an external command
build `CommandRequest` values, usually in `tmux` or `git`, and run them through
the local `command` module. User-facing interactions such as printing output or
running `fzf` stay in the CLI and feedback modules.

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
