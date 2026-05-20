# goose

<img width="1817" height="866" alt="goose" src="https://github.com/user-attachments/assets/27326804-4b41-4909-8e7e-631df4ef2b0f" />

<br>_Logo was a vaguely inspired by [house](https://github.com/house)._</br>


<br>A small Rust CLI that walks you through installing things, **it tells you what to type, but never runs it**. You stay in control of every command that touches your machine.</br>

```
  ╭──────╮
  │  ◔   │  goose — step-by-step install helper
  ╰──┬───╯  tells you what to type, never runs it
     ╰╮
```

## What it does

1. Asks what you're installing.
2. Detects your OS,  macOS, Windows, or one of Ubuntu / Debian / Fedora / Arch / generic Linux.
3. Walks you through the install one step at a time. Press Enter to advance, `b` to go back, `a` to show every step at once, `q` to quit.

The commands are printed inline, prefixed with `$`, ready to copy and paste.

## Built-in guides

`docker`, `node` (`nodejs`, `npm`, `nvm`), `rust` (`cargo`, `rustup`), `python` (`pip`), `git`, `postgres` (`postgresql`, `psql`), `nginx`, `go` (`golang`), `redis`, `homebrew` (`brew`).

Anything else falls back to your OS package manager (`brew search` → `brew install`, `apt-cache search` → `sudo apt install`, etc.).

## Build & run

```
cargo build --release
./target/release/goose
```

Zero dependencies beyond the Rust standard library.

<img width="1000" height="562" alt="goose-demo" src="https://github.com/user-attachments/assets/fb400aef-3c33-4755-984f-a4ba45e53176" />

## Why not just run the commands

Two reasons. First, `sudo` and shell history mean half the time the right move is to read the line, paste it into the right shell with the right environment, and watch what happens, not delegate to a tool. Second, install scripts evolve; this gives you the structure and lets you sanity check the latest URL or flag before you commit.

## Author

Michael Mendy (c) 2026. 
