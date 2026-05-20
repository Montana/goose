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
goose [OPTIONS] [TARGET]
```

Run interactively, or pass a target on the command line:

```
goose                       # interactive
goose docker                # jump straight to docker
goose --all postgres        # print every step at once (good for piping)
goose --os ubuntu kubectl   # pretend you're on a different distro
goose --list                # show known targets
goose --help
```

### Options

| Flag | Meaning |
| --- | --- |
| `-a`, `--all` | Print every step at once instead of walking through one by one |
| `-l`, `--list` | List known targets and exit |
| `--os <NAME>` | Override OS detection: `macos`, `ubuntu`, `debian`, `fedora`, `arch`, `linux`, `windows` |
| `--no-color` | Disable ANSI colors (also honored via the `NO_COLOR` env var) |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show version |

Color is also disabled automatically when stdout isn't a TTY, so `goose docker > steps.txt` produces a clean file.

## Built-in guides

`docker`, `node` (`nodejs`, `npm`, `nvm`), `rust` (`cargo`, `rustup`), `python` (`pip`), `git`, `postgres` (`postgresql`, `psql`), `nginx`, `go` (`golang`), `redis`, `homebrew` (`brew`), `kubectl` (`k8s`, `kubernetes`), `gh` (`github-cli`), `terraform`.

The Linux Go step is architecture-aware, it picks the right tarball for `x86_64`, `aarch64`, or `armv6` (Raspberry Pi).

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

Covers alias resolution, `/etc/os-release` parsing (including derivatives), argument parsing, and guide construction.

## Why not just run the commands

Two reasons. First, `sudo` and shell history mean half the time the right move is to read the line, paste it into the right shell with the right environment, and watch what happens, not delegate to a tool. Second, install scripts evolve; this gives you the structure and lets you sanity check the latest URL or flag before you commit.

## Author

Michael Mendy (c) 2026.
