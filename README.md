# goose

<img width="1817" height="866" alt="goose" src="https://github.com/user-attachments/assets/27326804-4b41-4909-8e7e-631df4ef2b0f" />

<br>_Logo was vaguely inspired by [house](https://github.com/house)._</br>

<br>A small Rust CLI that walks you through installing things, **it tells you what to type, but never runs it**. You stay in control of every command that touches your machine.</br>

```
  ╭──────╮
  │  ◔   │  goose: step-by-step install helper
  ╰──┬───╯
     ╰╮
```

## What it does

1. Asks (or accepts on the command line) what you're installing.
2. Detects your OS, macOS, Windows, or one of Ubuntu / Debian / Fedora / Arch / generic Linux. Derivatives like Pop!_OS, Mint, Raspbian, Manjaro, EndeavourOS, Rocky, and Alma are routed to the right family via `/etc/os-release`'s `ID` and `ID_LIKE`.
3. Walks you through the install one step at a time. Press Enter to advance, `b` to go back, `a` to show every step at once, `q` to quit.

The commands are printed inline, prefixed with `$`, ready to copy and paste.

<img width="1166" height="678" alt="Screenshot 2026-05-19 at 5 58 13 PM" src="https://github.com/user-attachments/assets/ea4b8202-68c9-4bbd-8f32-b002c5f8a394" />

## Usage

This is the basic usage of goose:

```
goose [OPTIONS] [TARGET]...
```

Run interactively, or pass one or more targets on the command line:

```
goose                              # interactive
goose docker                       # jump straight to docker
goose docker node redis            # walk through multiple targets in sequence
goose --preset web-dev             # expand a preset bundle (see --list)
goose --uninstall docker           # print uninstall commands instead
goose --check rust                 # 'do I already have it?' detection
goose --format markdown postgres   # paste-ready output for issues/PR descriptions
goose --format script -a docker > review.sh   # reviewable, all commands commented
goose --format json -a node | jq . # pipeable structured output
goose --search ngx                 # 'did you mean nginx?'
goose --shell fish rust            # rewrite a few lines for fish syntax
goose --os ubuntu kubectl          # pretend you're on a different distro
goose --all postgres               # print every step at once (good for piping)
goose --list                       # show known targets and presets
goose --help
```

### Options

| Flag | Meaning |
| --- | --- |
| `-a`, `--all` | Print every step at once instead of walking through one by one |
| `-l`, `--list` | List known targets and presets, then exit |
| `--search <QUERY>` | Fuzzy-search known targets (handles typos, suggests close matches) |
| `--preset <NAME>` | Expand a preset bundle (`web-dev`, `data-sci`, `devops`, `cli-power-user`, `backend`) |
| `--uninstall` | Print uninstall commands instead of install |
| `--check` | Print detection commands ('do I already have it?') |
| `--format <FMT>` | Output format: `text` (default), `markdown`, `json`, `script` |
| `--shell <SHELL>` | Override shell detection: `bash`, `zsh`, `fish`, `pwsh`, `cmd` |
| `--os <NAME>` | Override OS detection: `macos`, `ubuntu`, `debian`, `fedora`, `arch`, `linux`, `windows` |
| `--no-color` | Disable ANSI colors (also honored via the `NO_COLOR` env var) |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show version |

Color is also disabled automatically when stdout isn't a TTY (so `goose docker > steps.txt` produces a clean file) and in any non-text format.

### Output formats

- **`text`** — the default interactive walkthrough, or a colored dump with `--all`.
- **`markdown`** — paste-ready into a GitHub issue, PR description, or README. Each step becomes a `##` heading with a fenced code block; notes become blockquotes.
- **`json`** — one JSON object per target. Pipe through `jq` for tooling.
- **`script`** — a bash script with **every command commented out**. The whole point is that you read it, uncomment what you want, then run it deliberately. Same philosophy as the rest of goose: nothing executes without your say-so.

### Presets

Curated bundles, in install order (toolchains before things that depend on them):

| Preset | Contents |
| --- | --- |
| `web-dev` | git, node, python, docker, postgres |
| `data-sci` | git, python, postgres, redis |
| `devops` | docker, kubectl, helm, terraform, awscli |
| `cli-power-user` | fzf, ripgrep, bat, jq, tmux, neovim |
| `backend` | go, postgres, redis, docker |

You can also combine: `goose --preset web-dev redis` prepends the preset and adds redis at the end (deduping by canonical name).

### "Did you mean?"

If you type something that isn't a built-in guide but is close to one, goose mentions it before falling back to the package manager:

```
$ goose ngnix --os ubuntu --all
note: 'ngnix' isn't a built-in guide. did you mean: nginx, npm, nvm? falling back to your OS package manager.
```

It's a nudge, not a block — the fallback still runs in case you really did want `ngnix`.

## Built-in guides

Toolchains & runtimes: `docker`, `node` (`nodejs`, `npm`, `nvm`), `rust` (`cargo`, `rustup`), `python` (`pip`), `go` (`golang`), `bun`, `deno`, `pnpm`, `rbenv` (`ruby`).

Servers & data stores: `postgres` (`postgresql`, `psql`), `nginx`, `redis`.

Cloud & infra: `kubectl` (`k8s`, `kubernetes`), `helm`, `terraform`, `awscli` (`aws`, `aws-cli`), `docker`.

Dev tools & CLI quality-of-life: `git`, `gh` (`github-cli`), `homebrew` (`brew`), `neovim` (`nvim`, `vim`), `fzf`, `ripgrep` (`rg`), `bat`, `jq`, `tmux`.

The Linux Go step is architecture-aware — it picks the right tarball for `x86_64`, `aarch64`, or `armv6` (Raspberry Pi). The same applies to the Linux AWS CLI installer.

Anything else falls back to your OS package manager (`brew search` → `brew install`, `apt-cache search` → `sudo apt install`, etc.).

## Build & run

```
cargo build --release
./target/release/goose
```

Zero dependencies beyond the Rust standard library. Minimum Rust version: 1.70.

<img width="1000" height="562" alt="goose-demo" src="https://github.com/user-attachments/assets/fb400aef-3c33-4755-984f-a4ba45e53176" />

## Tests

```
cargo test
```

Covers alias resolution, `/etc/os-release` parsing (including derivatives), argument parsing (all flags), guide construction, fuzzy search ranking, shell adaptation, uninstall/check modes, and all three non-text renderers (markdown, JSON, script).

## Why not just run the commands

Two reasons. First, `sudo` and shell history mean half the time the right move is to read the line, paste it into the right shell with the right environment, and watch what happens, not delegate to a tool. Second, install scripts evolve; this gives you the structure and lets you sanity check the latest URL or flag before you commit.

## Author

Michael Mendy (c) 2026.
