use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::OnceLock;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
goose — step-by-step install helper

USAGE:
    goose [OPTIONS] [TARGET]...

ARGUMENTS:
    [TARGET]...    One or more things to install. Multiple targets walk
                   in sequence. If omitted, goose asks interactively.

OPTIONS:
    -a, --all              Print every step at once (skip the walkthrough)
    -l, --list             List known targets and presets, then exit
        --search <QUERY>   Find known targets matching QUERY (fuzzy)
        --preset <NAME>    Expand to a bundle of targets (see --list)
        --uninstall        Print uninstall commands instead of install
        --check            Print detection commands ('do I already have it?')
        --format <FMT>     Output format: text (default), markdown, json, script
        --shell <SHELL>    Override shell detection. One of:
                           bash, zsh, fish, pwsh, cmd
        --os <NAME>        Override OS detection. One of:
                           macos, ubuntu, debian, fedora, arch, linux, windows
        --no-color         Disable ANSI color (also: set NO_COLOR=1)
    -h, --help             Show this message and exit
    -V, --version          Show version and exit

EXAMPLES:
    goose docker
    goose docker node redis            # multi-target walkthrough
    goose --preset web-dev             # node + python + git + docker + postgres
    goose --uninstall docker
    goose --check rust
    goose --format markdown postgres   # paste-ready for issues/READMEs
    goose --format script -a docker > review.sh
    goose --search ngx                 # 'did you mean nginx?'

goose prints commands for you to copy and paste. It never runs them.
";

// ── Palette / TTY ──────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Palette {
    reset: &'static str,
    bold: &'static str,
    dim: &'static str,
    cyan: &'static str,
    green: &'static str,
    yellow: &'static str,
    magenta: &'static str,
}

impl Palette {
    const fn on() -> Self {
        Palette {
            reset: "\x1b[0m",
            bold: "\x1b[1m",
            dim: "\x1b[2m",
            cyan: "\x1b[36m",
            green: "\x1b[32m",
            yellow: "\x1b[33m",
            magenta: "\x1b[35m",
        }
    }
    const fn off() -> Self {
        Palette {
            reset: "",
            bold: "",
            dim: "",
            cyan: "",
            green: "",
            yellow: "",
            magenta: "",
        }
    }
}

static PALETTE: OnceLock<Palette> = OnceLock::new();

fn pal() -> &'static Palette {
    // If main() never set this (e.g. unit tests, library use), auto-detect.
    PALETTE.get_or_init(|| {
        if env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal() {
            Palette::on()
        } else {
            Palette::off()
        }
    })
}

// ── OS / arch ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum Os {
    MacOs,
    Ubuntu,
    Debian,
    Fedora,
    Arch,
    LinuxOther,
    Windows,
    Unknown,
}

impl Os {
    fn label(self) -> &'static str {
        match self {
            Os::MacOs => "macOS",
            Os::Ubuntu => "Ubuntu",
            Os::Debian => "Debian",
            Os::Fedora => "Fedora / RHEL family",
            Os::Arch => "Arch Linux",
            Os::LinuxOther => "Linux (generic)",
            Os::Windows => "Windows",
            Os::Unknown => "unknown OS",
        }
    }

    fn pkg_install_prefix(self) -> Option<&'static str> {
        match self {
            Os::MacOs => Some("brew install"),
            Os::Ubuntu | Os::Debian => Some("sudo apt install -y"),
            Os::Fedora => Some("sudo dnf install -y"),
            Os::Arch => Some("sudo pacman -S --noconfirm"),
            Os::Windows => Some("winget install -e --id"),
            _ => None,
        }
    }

    fn search_command(self, pkg: &str) -> String {
        match self {
            Os::MacOs => format!("brew search {pkg}"),
            Os::Ubuntu | Os::Debian => format!("apt-cache search {pkg}"),
            Os::Fedora => format!("dnf search {pkg}"),
            Os::Arch => format!("pacman -Ss {pkg}"),
            Os::Windows => format!("winget search {pkg}"),
            _ => format!("# search your package manager for {pkg}"),
        }
    }
}

fn detect_os() -> Os {
    match std::env::consts::OS {
        "macos" => Os::MacOs,
        "windows" => Os::Windows,
        "linux" => detect_linux(),
        _ => Os::Unknown,
    }
}

fn detect_linux() -> Os {
    let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let (id, id_like) = parse_os_release(&release);
    classify_linux(&id, &id_like)
}

/// Parse the `ID=` and `ID_LIKE=` fields out of `/etc/os-release` content.
/// Values may be quoted (`ID="rhel"`) or unquoted (`ID=ubuntu`); we strip
/// surrounding double/single quotes and lowercase the result.
fn parse_os_release(content: &str) -> (String, String) {
    let mut id = String::new();
    let mut id_like = String::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("ID=") {
            id = strip_quotes(val).to_lowercase();
        } else if let Some(val) = line.strip_prefix("ID_LIKE=") {
            id_like = strip_quotes(val).to_lowercase();
        }
    }
    (id, id_like)
}

fn strip_quotes(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Map `(ID, ID_LIKE)` into our coarse `Os` bucket. Checks `ID` first, then
/// each whitespace-separated entry of `ID_LIKE`. This is how derivatives
/// (Pop!_OS, Mint, Raspbian, EndeavourOS, Rocky, Alma, RHEL) land in the
/// right bucket.
fn classify_linux(id: &str, id_like: &str) -> Os {
    let id_like_parts: Vec<&str> = id_like.split_whitespace().collect();
    let is_like = |needle: &str| id == needle || id_like_parts.contains(&needle);

    if is_like("ubuntu") {
        return Os::Ubuntu;
    }
    if is_like("debian") {
        return Os::Debian;
    }
    if is_like("fedora") || is_like("rhel") || is_like("centos") {
        return Os::Fedora;
    }
    if is_like("arch") {
        return Os::Arch;
    }
    Os::LinuxOther
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arch {
    X86_64,
    Aarch64,
    Armv6,
    Other,
}

impl Arch {
    fn detect() -> Self {
        match std::env::consts::ARCH {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            "arm" => Arch::Armv6,
            _ => Arch::Other,
        }
    }

    /// Suffix used in the official Go download URLs (e.g. `linux-amd64`).
    fn go_linux_suffix(self) -> &'static str {
        match self {
            Arch::X86_64 => "linux-amd64",
            Arch::Aarch64 => "linux-arm64",
            Arch::Armv6 => "linux-armv6l",
            // Fall back to amd64; user should sanity-check the URL anyway.
            Arch::Other => "linux-amd64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OutputFormat {
    #[default]
    Text,
    Markdown,
    Json,
    Script,
}

fn parse_format(s: &str) -> Option<OutputFormat> {
    match s.trim().to_lowercase().as_str() {
        "text" | "txt" | "plain" => Some(OutputFormat::Text),
        "markdown" | "md" => Some(OutputFormat::Markdown),
        "json" => Some(OutputFormat::Json),
        "script" | "sh" | "bash" => Some(OutputFormat::Script),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
    Cmd,
}

impl Shell {
    /// Best-effort detection from $SHELL on unix, env on windows.
    /// We don't error if nothing's found — most steps don't care.
    fn detect() -> Option<Self> {
        if cfg!(windows) {
            // No reliable env var; lean on PSModulePath which is set inside
            // PowerShell. Otherwise default to cmd. Either way the
            // command bodies in our guides are almost all the same.
            if env::var_os("PSModulePath").is_some() {
                return Some(Shell::Pwsh);
            }
            return Some(Shell::Cmd);
        }
        let raw = env::var("SHELL").ok()?;
        let leaf = raw.rsplit('/').next().unwrap_or(&raw);
        match leaf {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "pwsh" | "powershell" => Some(Shell::Pwsh),
            _ => None,
        }
    }
}

fn parse_shell(s: &str) -> Option<Shell> {
    match s.trim().to_lowercase().as_str() {
        "bash" | "sh" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        "pwsh" | "powershell" => Some(Shell::Pwsh),
        "cmd" => Some(Shell::Cmd),
        _ => None,
    }
}

// ── CLI args ───────────────────────────────────────────────────────────────

#[derive(Default, Debug, PartialEq, Eq)]
struct Args {
    targets: Vec<String>,
    show_all: bool,
    list: bool,
    os_override: Option<Os>,
    shell_override: Option<Shell>,
    no_color: bool,
    show_help: bool,
    show_version: bool,
    format: OutputFormat,
    uninstall: bool,
    check_only: bool,
    search: Option<String>,
    preset: Option<String>,
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut out = Args::default();
    let mut iter = args.into_iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "-h" | "--help" => out.show_help = true,
            "-V" | "--version" => out.show_version = true,
            "-a" | "--all" => out.show_all = true,
            "-l" | "--list" => out.list = true,
            "--no-color" => out.no_color = true,
            "--uninstall" => out.uninstall = true,
            "--check" => out.check_only = true,
            "--os" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--os requires a value".to_string())?;
                out.os_override = Some(parse_os(&v).ok_or_else(|| format!("unknown OS: {v}"))?);
            }
            s if s.starts_with("--os=") => {
                let v = &s[5..];
                out.os_override = Some(parse_os(v).ok_or_else(|| format!("unknown OS: {v}"))?);
            }
            "--shell" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--shell requires a value".to_string())?;
                out.shell_override =
                    Some(parse_shell(&v).ok_or_else(|| format!("unknown shell: {v}"))?);
            }
            s if s.starts_with("--shell=") => {
                let v = &s[8..];
                out.shell_override =
                    Some(parse_shell(v).ok_or_else(|| format!("unknown shell: {v}"))?);
            }
            "--format" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                out.format = parse_format(&v).ok_or_else(|| format!("unknown format: {v}"))?;
            }
            s if s.starts_with("--format=") => {
                let v = &s[9..];
                out.format = parse_format(v).ok_or_else(|| format!("unknown format: {v}"))?;
            }
            "--search" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--search requires a value".to_string())?;
                out.search = Some(v);
            }
            s if s.starts_with("--search=") => {
                out.search = Some(s[9..].to_string());
            }
            "--preset" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--preset requires a value".to_string())?;
                out.preset = Some(v);
            }
            s if s.starts_with("--preset=") => {
                out.preset = Some(s[9..].to_string());
            }
            "--" => {
                // Everything after `--` is positional.
                for rest in iter.by_ref() {
                    out.targets.push(rest);
                }
            }
            s if s.starts_with("--") => {
                return Err(format!("unknown option: {s}"));
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!("unknown option: {s}"));
            }
            _ => {
                out.targets.push(a);
            }
        }
    }
    Ok(out)
}

fn parse_os(s: &str) -> Option<Os> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "macos" | "mac" | "osx" | "darwin" => Some(Os::MacOs),
        "ubuntu" => Some(Os::Ubuntu),
        "debian" => Some(Os::Debian),
        "fedora" | "rhel" | "centos" | "rocky" | "alma" | "almalinux" => Some(Os::Fedora),
        "arch" | "manjaro" | "endeavouros" => Some(Os::Arch),
        "linux" => Some(Os::LinuxOther),
        "windows" | "win" => Some(Os::Windows),
        _ => None,
    }
}

// ── Steps & guides ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct Step {
    title: String,
    command: Option<String>,
    note: Option<String>,
}

fn s(title: &str, cmd: &str) -> Step {
    Step {
        title: title.into(),
        command: Some(cmd.into()),
        note: None,
    }
}

fn sn(title: &str, cmd: &str, note: &str) -> Step {
    Step {
        title: title.into(),
        command: Some(cmd.into()),
        note: Some(note.into()),
    }
}

fn info(title: &str, note: &str) -> Step {
    Step {
        title: title.into(),
        command: None,
        note: Some(note.into()),
    }
}

struct Guide {
    target: String,
    os: Os,
    steps: Vec<Step>,
}

/// Known targets, with their aliases and a one-line description.
/// Single source of truth for both `canonical()` and `--list`.
const KNOWN_TARGETS: &[(&str, &[&str], &str)] = &[
    (
        "docker",
        &["docker-compose"],
        "Docker Engine / Docker Desktop",
    ),
    (
        "nodejs",
        &["node", "npm", "nvm"],
        "Node.js (via nvm) and npm",
    ),
    (
        "rust",
        &["cargo", "rustup"],
        "Rust toolchain (rustup, cargo)",
    ),
    ("python", &["python3", "pip", "pip3"], "Python 3 and pip"),
    ("git", &[], "Git"),
    ("postgresql", &["postgres", "psql"], "PostgreSQL"),
    ("nginx", &[], "nginx web server"),
    ("go", &["golang"], "Go toolchain"),
    ("redis", &[], "Redis"),
    ("homebrew", &["brew"], "Homebrew package manager"),
    ("kubectl", &["k8s", "kubernetes"], "Kubernetes CLI"),
    ("gh", &["github-cli"], "GitHub CLI"),
    ("terraform", &[], "Terraform"),
    ("bun", &[], "Bun JS runtime + package manager"),
    ("deno", &[], "Deno JS/TS runtime"),
    ("pnpm", &[], "pnpm package manager"),
    ("neovim", &["nvim", "vim"], "Neovim editor"),
    ("fzf", &[], "Fuzzy finder for the shell"),
    ("ripgrep", &["rg"], "Recursive code search (rg)"),
    ("bat", &[], "cat clone with syntax highlighting"),
    ("jq", &[], "Command-line JSON processor"),
    ("helm", &[], "Kubernetes package manager"),
    ("awscli", &["aws", "aws-cli"], "AWS command-line interface"),
    ("tmux", &[], "Terminal multiplexer"),
    ("rbenv", &["ruby"], "Ruby version manager"),
];

fn canonical(input: &str) -> String {
    let key = input.trim().to_lowercase();
    for (canon, aliases, _) in KNOWN_TARGETS {
        if key == *canon {
            return (*canon).to_string();
        }
        for alias in *aliases {
            if key == *alias {
                return (*canon).to_string();
            }
        }
    }
    key
}

fn build_guide(input: &str, os: Os) -> Guide {
    let key = canonical(input);
    let steps = match key.as_str() {
        "docker" => docker_steps(os),
        "nodejs" => nodejs_steps(os),
        "rust" => rust_steps(os),
        "python" => python_steps(os),
        "git" => git_steps(os),
        "postgresql" => postgres_steps(os),
        "nginx" => nginx_steps(os),
        "go" => go_steps(os),
        "redis" => redis_steps(os),
        "homebrew" => homebrew_steps(os),
        "kubectl" => kubectl_steps(os),
        "gh" => gh_steps(os),
        "terraform" => terraform_steps(os),
        "bun" => bun_steps(os),
        "deno" => deno_steps(os),
        "pnpm" => pnpm_steps(os),
        "neovim" => neovim_steps(os),
        "fzf" => fzf_steps(os),
        "ripgrep" => ripgrep_steps(os),
        "bat" => bat_steps(os),
        "jq" => jq_steps(os),
        "helm" => helm_steps(os),
        "awscli" => awscli_steps(os),
        "tmux" => tmux_steps(os),
        "rbenv" => rbenv_steps(os),
        _ => generic_steps(input, os),
    };
    Guide {
        target: input.trim().to_string(),
        os,
        steps,
    }
}

// ── Individual package guides ──────────────────────────────────────────────

fn docker_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            sn(
                "Check that Homebrew is installed",
                "brew --version",
                "If 'command not found', run goose again and ask for 'homebrew' first.",
            ),
            s("Install Docker Desktop", "brew install --cask docker"),
            info(
                "Launch Docker Desktop from Applications",
                "Open it once so the daemon can start; you'll be asked for your password.",
            ),
            s("Verify the install", "docker run --rm hello-world"),
        ],
        Os::Ubuntu | Os::Debian => {
            let distro = if os == Os::Debian { "debian" } else { "ubuntu" };
            vec![
                s(
                    "Remove any old Docker packages",
                    "sudo apt remove -y docker docker-engine docker.io containerd runc || true",
                ),
                s(
                    "Update apt and install prerequisites",
                    "sudo apt update && sudo apt install -y ca-certificates curl gnupg",
                ),
                sn(
                    "Add Docker's official GPG key",
                    &format!(
                        "sudo install -m 0755 -d /etc/apt/keyrings && \\\n  curl -fsSL https://download.docker.com/linux/{distro}/gpg | \\\n  sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg && \\\n  sudo chmod a+r /etc/apt/keyrings/docker.gpg"
                    ),
                    "This is one logical command split across three lines with backslashes.",
                ),
                s(
                    "Add the Docker repository to apt sources",
                    &format!(
                        "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/{distro} $(. /etc/os-release && echo \\\"$VERSION_CODENAME\\\") stable\" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null"
                    ),
                ),
                s(
                    "Install Docker Engine",
                    "sudo apt update && sudo apt install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
                ),
                sn(
                    "Allow your user to run docker without sudo",
                    "sudo usermod -aG docker $USER",
                    "Log out and back in (or run 'newgrp docker') for the group change to apply.",
                ),
                s("Verify the install", "docker run --rm hello-world"),
            ]
        }
        Os::Fedora => vec![
            s(
                "Remove any old Docker packages",
                "sudo dnf remove -y docker docker-client docker-client-latest docker-common docker-latest docker-latest-logrotate docker-logrotate docker-engine || true",
            ),
            s(
                "Add the Docker repo",
                "sudo dnf -y install dnf-plugins-core && \\\n  sudo dnf config-manager --add-repo https://download.docker.com/linux/fedora/docker-ce.repo",
            ),
            s(
                "Install Docker Engine",
                "sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
            ),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now docker",
            ),
            sn(
                "Allow your user to run docker without sudo",
                "sudo usermod -aG docker $USER",
                "Log out and back in for the group change to apply.",
            ),
            s("Verify the install", "sudo docker run --rm hello-world"),
        ],
        Os::Arch => vec![
            s(
                "Install Docker",
                "sudo pacman -S --noconfirm docker docker-compose",
            ),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now docker.service",
            ),
            sn(
                "Allow your user to run docker without sudo",
                "sudo usermod -aG docker $USER",
                "Log out and back in for this to take effect.",
            ),
            s("Verify the install", "docker run --rm hello-world"),
        ],
        Os::Windows => vec![
            info(
                "Docker on Windows ships as a desktop app",
                "Easiest path is winget; WSL2 is required and the installer will set it up.",
            ),
            sn(
                "Install via winget",
                "winget install -e --id Docker.DockerDesktop",
                "Reboot when prompted.",
            ),
            info(
                "Launch Docker Desktop from the Start menu",
                "Wait until the whale icon stops animating.",
            ),
            s("Verify the install", "docker run --rm hello-world"),
        ],
        _ => generic_steps("docker", os),
    }
}

fn nodejs_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs | Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => vec![
            sn(
                "Install nvm (Node Version Manager)",
                "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash",
                "nvm lets you switch between Node versions per project. Avoids the system's stale 'nodejs' package.",
            ),
            sn(
                "Reload your shell so nvm is on PATH",
                "exec $SHELL -l",
                "Or just open a new terminal tab.",
            ),
            s("Install the latest LTS Node", "nvm install --lts"),
            s("Verify", "node --version && npm --version"),
        ],
        Os::Arch => vec![
            s(
                "Install Node and npm",
                "sudo pacman -S --noconfirm nodejs npm",
            ),
            s("Verify", "node --version && npm --version"),
        ],
        Os::Windows => vec![
            sn(
                "Install nvm-windows",
                "winget install -e --id CoreyButler.NVMforWindows",
                "Open a new terminal after install so nvm is on PATH.",
            ),
            s("Install the latest LTS Node", "nvm install lts"),
            s("Activate it", "nvm use lts"),
            s("Verify", "node --version && npm --version"),
        ],
        _ => generic_steps("nodejs", os),
    }
}

fn rust_steps(os: Os) -> Vec<Step> {
    match os {
        Os::Windows => vec![
            info(
                "Download rustup-init.exe",
                "Visit https://rustup.rs and run the installer.",
            ),
            info(
                "Accept the defaults",
                "It will install MSVC build tools if they're missing.",
            ),
            s(
                "Open a new terminal and verify",
                "rustc --version && cargo --version",
            ),
        ],
        Os::Unknown => generic_steps("rust", os),
        _ => vec![
            sn(
                "Install rustup (the official Rust toolchain installer)",
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
                "Accept the default install when prompted.",
            ),
            s(
                "Source the new cargo env into your current shell",
                ". \"$HOME/.cargo/env\"",
            ),
            s("Verify", "rustc --version && cargo --version"),
        ],
    }
}

fn python_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            sn(
                "Install Python via Homebrew",
                "brew install python",
                "macOS ships an old Python; Homebrew's is current and writable.",
            ),
            s("Verify", "python3 --version && pip3 --version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s("Update apt", "sudo apt update"),
            s(
                "Install Python 3, pip, and venv",
                "sudo apt install -y python3 python3-pip python3-venv",
            ),
            s("Verify", "python3 --version && pip3 --version"),
        ],
        Os::Fedora => vec![
            s(
                "Install Python 3 and pip",
                "sudo dnf install -y python3 python3-pip",
            ),
            s("Verify", "python3 --version && pip3 --version"),
        ],
        Os::Arch => vec![
            s(
                "Install Python",
                "sudo pacman -S --noconfirm python python-pip",
            ),
            s("Verify", "python --version && pip --version"),
        ],
        Os::Windows => {
            vec![
            sn(
                "Install Python via winget",
                "winget install -e --id Python.Python.3.12",
                "If you use the GUI installer instead, make sure 'Add Python to PATH' is checked.",
            ),
            s("Verify in a new terminal", "python --version && pip --version"),
        ]
        }
        _ => generic_steps("python", os),
    }
}

fn git_steps(os: Os) -> Vec<Step> {
    let install = match os {
        Os::MacOs => s("Install Git", "brew install git"),
        Os::Ubuntu | Os::Debian => s("Install Git", "sudo apt update && sudo apt install -y git"),
        Os::Fedora => s("Install Git", "sudo dnf install -y git"),
        Os::Arch => s("Install Git", "sudo pacman -S --noconfirm git"),
        Os::Windows => sn(
            "Install Git for Windows",
            "winget install -e --id Git.Git",
            "Includes Git Bash, a Unix-ish shell that's handy on Windows.",
        ),
        _ => return generic_steps("git", os),
    };
    vec![
        install,
        s("Verify", "git --version"),
        sn(
            "Set your name",
            "git config --global user.name \"Your Name\"",
            "Replace 'Your Name' with what you want commits attributed to.",
        ),
        sn(
            "Set your email",
            "git config --global user.email \"you@example.com\"",
            "Use the email tied to your GitHub/GitLab account.",
        ),
        sn(
            "Pick a default branch name",
            "git config --global init.defaultBranch main",
            "Optional but conventional these days.",
        ),
    ]
}

fn postgres_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install PostgreSQL", "brew install postgresql@16"),
            sn(
                "Start the service",
                "brew services start postgresql@16",
                "Runs Postgres in the background and on login.",
            ),
            s("Verify", "psql --version"),
            sn(
                "Create your first database",
                "createdb mydb",
                "createdb is a wrapper around `psql -c 'CREATE DATABASE mydb'`.",
            ),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install PostgreSQL",
                "sudo apt update && sudo apt install -y postgresql postgresql-contrib",
            ),
            s(
                "Make sure the service is running",
                "sudo systemctl enable --now postgresql",
            ),
            sn(
                "Connect as the postgres superuser",
                "sudo -u postgres psql",
                "Type \\q to exit. From here you can `CREATE USER ...` and `CREATE DATABASE ...`.",
            ),
        ],
        Os::Fedora => vec![
            s(
                "Install PostgreSQL",
                "sudo dnf install -y postgresql-server postgresql-contrib",
            ),
            s(
                "Initialize the data directory",
                "sudo postgresql-setup --initdb",
            ),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now postgresql",
            ),
            sn(
                "Connect as the postgres superuser",
                "sudo -u postgres psql",
                "Type \\q to exit.",
            ),
        ],
        Os::Arch => vec![
            s(
                "Install PostgreSQL",
                "sudo pacman -S --noconfirm postgresql",
            ),
            sn(
                "Initialize the data directory as the postgres user",
                "sudo -iu postgres initdb -D /var/lib/postgres/data",
                "Required before the service will start.",
            ),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now postgresql",
            ),
        ],
        Os::Windows => vec![
            sn(
                "Install PostgreSQL via winget",
                "winget install -e --id PostgreSQL.PostgreSQL.16",
                "You'll set a superuser password during install — remember it.",
            ),
            info(
                "Use pgAdmin or psql from the Start menu",
                "The installer adds both.",
            ),
        ],
        _ => generic_steps("postgresql", os),
    }
}

fn nginx_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install nginx", "brew install nginx"),
            sn(
                "Start it",
                "brew services start nginx",
                "On macOS the default port is 8080, not 80.",
            ),
            info(
                "Open http://localhost:8080 in a browser",
                "You should see the nginx welcome page.",
            ),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install nginx",
                "sudo apt update && sudo apt install -y nginx",
            ),
            s(
                "Make sure it's running",
                "sudo systemctl enable --now nginx",
            ),
            sn(
                "Test it",
                "curl http://localhost",
                "Or visit http://localhost in a browser.",
            ),
        ],
        Os::Fedora => vec![
            s("Install nginx", "sudo dnf install -y nginx"),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now nginx",
            ),
            sn(
                "Open firewall ports (if firewalld is active)",
                "sudo firewall-cmd --permanent --add-service=http && sudo firewall-cmd --reload",
                "Skip if you're only testing locally.",
            ),
        ],
        Os::Arch => vec![
            s("Install nginx", "sudo pacman -S --noconfirm nginx"),
            s(
                "Enable and start the service",
                "sudo systemctl enable --now nginx",
            ),
        ],
        _ => generic_steps("nginx", os),
    }
}

fn go_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install Go", "brew install go"),
            s("Verify", "go version"),
        ],
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => {
            let arch = Arch::detect();
            let suffix = arch.go_linux_suffix();
            let tarball = format!("go1.22.0.{suffix}.tar.gz");
            let url = format!("https://go.dev/dl/{tarball}");
            let arch_note = if matches!(arch, Arch::Other) {
                "Couldn't detect your CPU arch; defaulting to amd64. Visit https://go.dev/dl to pick the right one."
            } else {
                "Check https://go.dev/dl for newer versions; the URL changes over time."
            };
            vec![
                sn(
                    "Download the Go tarball",
                    &format!("curl -fsSL -O {url}"),
                    arch_note,
                ),
                s(
                    "Replace any existing /usr/local/go install",
                    &format!("sudo rm -rf /usr/local/go && sudo tar -C /usr/local -xzf {tarball}"),
                ),
                sn(
                    "Add Go to your PATH",
                    "echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.profile",
                    "Open a new shell or run 'source ~/.profile' for it to take effect.",
                ),
                s("Verify", "go version"),
            ]
        }
        Os::Arch => vec![
            s("Install Go", "sudo pacman -S --noconfirm go"),
            s("Verify", "go version"),
        ],
        Os::Windows => vec![
            s("Install Go via winget", "winget install -e --id GoLang.Go"),
            s("Verify in a new terminal", "go version"),
        ],
        _ => generic_steps("go", os),
    }
}

fn redis_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install Redis", "brew install redis"),
            s("Start the service", "brew services start redis"),
            sn("Verify", "redis-cli ping", "You should see 'PONG'."),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install Redis",
                "sudo apt update && sudo apt install -y redis-server",
            ),
            s(
                "Make sure it's running",
                "sudo systemctl enable --now redis-server",
            ),
            sn("Verify", "redis-cli ping", "You should see 'PONG'."),
        ],
        Os::Fedora => vec![
            s("Install Redis", "sudo dnf install -y redis"),
            s("Enable and start", "sudo systemctl enable --now redis"),
            sn("Verify", "redis-cli ping", "You should see 'PONG'."),
        ],
        Os::Arch => vec![
            s("Install Redis", "sudo pacman -S --noconfirm redis"),
            s("Enable and start", "sudo systemctl enable --now redis"),
            sn("Verify", "redis-cli ping", "You should see 'PONG'."),
        ],
        _ => generic_steps("redis", os),
    }
}

fn homebrew_steps(os: Os) -> Vec<Step> {
    if matches!(os, Os::Windows | Os::Unknown) {
        return vec![info(
            "Homebrew is for macOS (and works on Linux)",
            "On Windows, use winget or scoop instead.",
        )];
    }
    let path_hint = if os == Os::MacOs {
        "On Intel Macs replace /opt/homebrew with /usr/local."
    } else {
        "On Linux, brew installs to /home/linuxbrew/.linuxbrew."
    };
    vec![
        sn(
            "Install Homebrew",
            "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"",
            "Runs the official install script over HTTPS. Review it first if you're cautious.",
        ),
        sn(
            "Add brew to your shell's PATH",
            "echo 'eval \"$(/opt/homebrew/bin/brew shellenv)\"' >> ~/.zprofile && eval \"$(/opt/homebrew/bin/brew shellenv)\"",
            path_hint,
        ),
        s("Verify", "brew --version"),
    ]
}

fn kubectl_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install kubectl", "brew install kubectl"),
            s("Verify", "kubectl version --client"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install prerequisites",
                "sudo apt update && sudo apt install -y apt-transport-https ca-certificates curl gnupg",
            ),
            sn(
                "Add the Kubernetes signing key",
                "curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.30/deb/Release.key | \\\n  sudo gpg --dearmor -o /etc/apt/keyrings/kubernetes-apt-keyring.gpg",
                "Pinning to v1.30; bump the URL when a newer minor is current.",
            ),
            s(
                "Add the Kubernetes apt repo",
                "echo 'deb [signed-by=/etc/apt/keyrings/kubernetes-apt-keyring.gpg] https://pkgs.k8s.io/core:/stable:/v1.30/deb/ /' | sudo tee /etc/apt/sources.list.d/kubernetes.list",
            ),
            s(
                "Install kubectl",
                "sudo apt update && sudo apt install -y kubectl",
            ),
            s("Verify", "kubectl version --client"),
        ],
        Os::Fedora => vec![
            sn(
                "Add the Kubernetes dnf repo",
                "cat <<'EOF' | sudo tee /etc/yum.repos.d/kubernetes.repo\n[kubernetes]\nname=Kubernetes\nbaseurl=https://pkgs.k8s.io/core:/stable:/v1.30/rpm/\nenabled=1\ngpgcheck=1\ngpgkey=https://pkgs.k8s.io/core:/stable:/v1.30/rpm/repodata/repomd.xml.key\nEOF",
                "This is one heredoc block; copy the whole thing.",
            ),
            s("Install kubectl", "sudo dnf install -y kubectl"),
            s("Verify", "kubectl version --client"),
        ],
        Os::Arch => vec![
            s("Install kubectl", "sudo pacman -S --noconfirm kubectl"),
            s("Verify", "kubectl version --client"),
        ],
        Os::Windows => vec![
            s(
                "Install kubectl via winget",
                "winget install -e --id Kubernetes.kubectl",
            ),
            s("Verify in a new terminal", "kubectl version --client"),
        ],
        _ => generic_steps("kubectl", os),
    }
}

fn gh_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install the GitHub CLI", "brew install gh"),
            sn(
                "Sign in",
                "gh auth login",
                "Interactive; pick GitHub.com and HTTPS unless you know you want otherwise.",
            ),
        ],
        Os::Ubuntu | Os::Debian => vec![
            sn(
                "Add the GitHub CLI signing key",
                "curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | \\\n  sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg && \\\n  sudo chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg",
                "One logical command, split across lines with backslashes.",
            ),
            s(
                "Add the apt repo",
                "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null",
            ),
            s(
                "Install gh",
                "sudo apt update && sudo apt install -y gh",
            ),
            sn(
                "Sign in",
                "gh auth login",
                "Pick GitHub.com and HTTPS unless you know you want otherwise.",
            ),
        ],
        Os::Fedora => vec![
            s(
                "Add the GitHub CLI repo",
                "sudo dnf config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo",
            ),
            s("Install gh", "sudo dnf install -y gh"),
            s("Sign in", "gh auth login"),
        ],
        Os::Arch => vec![
            s("Install gh", "sudo pacman -S --noconfirm github-cli"),
            s("Sign in", "gh auth login"),
        ],
        Os::Windows => vec![
            s(
                "Install via winget",
                "winget install -e --id GitHub.cli",
            ),
            s("Sign in (in a new terminal)", "gh auth login"),
        ],
        _ => generic_steps("gh", os),
    }
}

fn terraform_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Tap HashiCorp's brew formulas", "brew tap hashicorp/tap"),
            s(
                "Install Terraform",
                "brew install hashicorp/tap/terraform",
            ),
            s("Verify", "terraform -version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install prerequisites",
                "sudo apt update && sudo apt install -y gnupg software-properties-common curl",
            ),
            s(
                "Add HashiCorp's signing key",
                "wget -O- https://apt.releases.hashicorp.com/gpg | \\\n  gpg --dearmor | \\\n  sudo tee /usr/share/keyrings/hashicorp-archive-keyring.gpg > /dev/null",
            ),
            s(
                "Add the apt repo",
                "echo \"deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main\" | sudo tee /etc/apt/sources.list.d/hashicorp.list",
            ),
            s(
                "Install Terraform",
                "sudo apt update && sudo apt install -y terraform",
            ),
            s("Verify", "terraform -version"),
        ],
        Os::Fedora => vec![
            s(
                "Add HashiCorp's dnf repo",
                "sudo dnf install -y dnf-plugins-core && \\\n  sudo dnf config-manager --add-repo https://rpm.releases.hashicorp.com/fedora/hashicorp.repo",
            ),
            s("Install Terraform", "sudo dnf install -y terraform"),
            s("Verify", "terraform -version"),
        ],
        Os::Arch => vec![
            s(
                "Install Terraform",
                "sudo pacman -S --noconfirm terraform",
            ),
            s("Verify", "terraform -version"),
        ],
        Os::Windows => vec![
            s(
                "Install via winget",
                "winget install -e --id Hashicorp.Terraform",
            ),
            s("Verify in a new terminal", "terraform -version"),
        ],
        _ => generic_steps("terraform", os),
    }
}

// ── New step builders ──────────────────────────────────────────────────────

fn bun_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs | Os::Ubuntu | Os::Debian | Os::Fedora | Os::Arch | Os::LinuxOther => vec![
            sn(
                "Run Bun's install script",
                "curl -fsSL https://bun.sh/install | bash",
                "Drops `bun` into ~/.bun/bin; the script edits your shell rc to add it to PATH.",
            ),
            sn(
                "Reload your shell so bun is on PATH",
                "exec $SHELL -l",
                "Or open a new terminal tab.",
            ),
            s("Verify", "bun --version"),
        ],
        Os::Windows => vec![
            sn(
                "Install Bun in PowerShell",
                "powershell -c \"irm bun.sh/install.ps1 | iex\"",
                "Run this from PowerShell, not cmd.exe. Open a new terminal after.",
            ),
            s("Verify in a new terminal", "bun --version"),
        ],
        _ => generic_steps("bun", os),
    }
}

fn deno_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install Deno via Homebrew", "brew install deno"),
            s("Verify", "deno --version"),
        ],
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::Arch | Os::LinuxOther => vec![
            sn(
                "Run Deno's install script",
                "curl -fsSL https://deno.land/install.sh | sh",
                "Drops `deno` into ~/.deno/bin; you'll need to add that to PATH.",
            ),
            sn(
                "Add Deno to your PATH",
                "echo 'export DENO_INSTALL=\"$HOME/.deno\"' >> ~/.profile && \\\n  echo 'export PATH=\"$DENO_INSTALL/bin:$PATH\"' >> ~/.profile",
                "Open a new shell after, or source ~/.profile.",
            ),
            s("Verify", "deno --version"),
        ],
        Os::Windows => vec![
            s("Install Deno via winget", "winget install -e --id DenoLand.Deno"),
            s("Verify in a new terminal", "deno --version"),
        ],
        _ => generic_steps("deno", os),
    }
}

fn pnpm_steps(os: Os) -> Vec<Step> {
    match os {
        Os::Windows => vec![
            sn(
                "Install pnpm via winget",
                "winget install -e --id pnpm.pnpm",
                "Or, if you already have node: `corepack enable && corepack prepare pnpm@latest --activate`.",
            ),
            s("Verify in a new terminal", "pnpm --version"),
        ],
        Os::MacOs | Os::Ubuntu | Os::Debian | Os::Fedora | Os::Arch | Os::LinuxOther => vec![
            sn(
                "Install pnpm via the standalone script",
                "curl -fsSL https://get.pnpm.io/install.sh | sh -",
                "If you already have Node, `corepack enable && corepack prepare pnpm@latest --activate` is cleaner.",
            ),
            sn(
                "Reload your shell so pnpm is on PATH",
                "exec $SHELL -l",
                "Or open a new terminal tab.",
            ),
            s("Verify", "pnpm --version"),
        ],
        _ => generic_steps("pnpm", os),
    }
}

fn neovim_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install Neovim", "brew install neovim"),
            s("Verify", "nvim --version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            sn(
                "Install Neovim",
                "sudo apt update && sudo apt install -y neovim",
                "Ubuntu's package can lag behind upstream; for the latest, use the AppImage from https://github.com/neovim/neovim/releases.",
            ),
            s("Verify", "nvim --version"),
        ],
        Os::Fedora => vec![
            s("Install Neovim", "sudo dnf install -y neovim"),
            s("Verify", "nvim --version"),
        ],
        Os::Arch => vec![
            s("Install Neovim", "sudo pacman -S --noconfirm neovim"),
            s("Verify", "nvim --version"),
        ],
        Os::Windows => vec![
            s("Install via winget", "winget install -e --id Neovim.Neovim"),
            s("Verify in a new terminal", "nvim --version"),
        ],
        _ => generic_steps("neovim", os),
    }
}

fn fzf_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install fzf", "brew install fzf"),
            sn(
                "Install key bindings and fuzzy completion",
                "$(brew --prefix)/opt/fzf/install",
                "Interactive; says yes/no for shell integration, history, completion.",
            ),
        ],
        Os::Ubuntu | Os::Debian => vec![
            sn(
                "Install fzf",
                "sudo apt update && sudo apt install -y fzf",
                "For the latest version + key bindings, use the git installer at https://github.com/junegunn/fzf.",
            ),
        ],
        Os::Fedora => vec![s("Install fzf", "sudo dnf install -y fzf")],
        Os::Arch => vec![s("Install fzf", "sudo pacman -S --noconfirm fzf")],
        Os::Windows => vec![s("Install via winget", "winget install -e --id junegunn.fzf")],
        _ => generic_steps("fzf", os),
    }
}

fn ripgrep_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install ripgrep", "brew install ripgrep"),
            s("Verify", "rg --version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s(
                "Install ripgrep",
                "sudo apt update && sudo apt install -y ripgrep",
            ),
            s("Verify", "rg --version"),
        ],
        Os::Fedora => vec![
            s("Install ripgrep", "sudo dnf install -y ripgrep"),
            s("Verify", "rg --version"),
        ],
        Os::Arch => vec![
            s("Install ripgrep", "sudo pacman -S --noconfirm ripgrep"),
            s("Verify", "rg --version"),
        ],
        Os::Windows => vec![
            s(
                "Install via winget",
                "winget install -e --id BurntSushi.ripgrep.MSVC",
            ),
            s("Verify in a new terminal", "rg --version"),
        ],
        _ => generic_steps("ripgrep", os),
    }
}

fn bat_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install bat", "brew install bat"),
            s("Verify", "bat --version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            sn(
                "Install bat",
                "sudo apt update && sudo apt install -y bat",
                "On Debian/Ubuntu the binary is named `batcat`. Alias it: `mkdir -p ~/.local/bin && ln -s /usr/bin/batcat ~/.local/bin/bat`.",
            ),
        ],
        Os::Fedora => vec![s("Install bat", "sudo dnf install -y bat")],
        Os::Arch => vec![s("Install bat", "sudo pacman -S --noconfirm bat")],
        Os::Windows => vec![s("Install via winget", "winget install -e --id sharkdp.bat")],
        _ => generic_steps("bat", os),
    }
}

fn jq_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install jq", "brew install jq"),
            s("Verify", "jq --version"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s("Install jq", "sudo apt update && sudo apt install -y jq"),
            s("Verify", "jq --version"),
        ],
        Os::Fedora => vec![
            s("Install jq", "sudo dnf install -y jq"),
            s("Verify", "jq --version"),
        ],
        Os::Arch => vec![
            s("Install jq", "sudo pacman -S --noconfirm jq"),
            s("Verify", "jq --version"),
        ],
        Os::Windows => vec![
            s("Install via winget", "winget install -e --id jqlang.jq"),
            s("Verify in a new terminal", "jq --version"),
        ],
        _ => generic_steps("jq", os),
    }
}

fn helm_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install Helm", "brew install helm"),
            s("Verify", "helm version"),
        ],
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => vec![
            sn(
                "Run Helm's install script",
                "curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash",
                "The script downloads the right binary for your arch and installs to /usr/local/bin.",
            ),
            s("Verify", "helm version"),
        ],
        Os::Arch => vec![
            s("Install Helm", "sudo pacman -S --noconfirm helm"),
            s("Verify", "helm version"),
        ],
        Os::Windows => vec![
            s(
                "Install via winget",
                "winget install -e --id Helm.Helm",
            ),
            s("Verify in a new terminal", "helm version"),
        ],
        _ => generic_steps("helm", os),
    }
}

fn awscli_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install the AWS CLI", "brew install awscli"),
            s("Verify", "aws --version"),
            sn(
                "Configure your credentials",
                "aws configure",
                "Interactive; prompts for access key, secret, default region, output format.",
            ),
        ],
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => {
            let arch = Arch::detect();
            let url_suffix = match arch {
                Arch::Aarch64 => "aarch64",
                _ => "x86_64",
            };
            vec![
                s(
                    "Download the official installer",
                    &format!(
                        "curl -fsSL \"https://awscli.amazonaws.com/awscli-exe-linux-{url_suffix}.zip\" -o awscliv2.zip"
                    ),
                ),
                s(
                    "Unzip and install",
                    "unzip awscliv2.zip && sudo ./aws/install",
                ),
                s("Verify", "aws --version"),
                sn(
                    "Configure your credentials",
                    "aws configure",
                    "Prompts for access key, secret, default region, output format.",
                ),
            ]
        }
        Os::Arch => vec![
            s("Install the AWS CLI", "sudo pacman -S --noconfirm aws-cli"),
            s("Verify", "aws --version"),
        ],
        Os::Windows => vec![
            s("Install via winget", "winget install -e --id Amazon.AWSCLI"),
            s("Verify in a new terminal", "aws --version"),
        ],
        _ => generic_steps("awscli", os),
    }
}

fn tmux_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install tmux", "brew install tmux"),
            s("Verify", "tmux -V"),
        ],
        Os::Ubuntu | Os::Debian => vec![
            s("Install tmux", "sudo apt update && sudo apt install -y tmux"),
            s("Verify", "tmux -V"),
        ],
        Os::Fedora => vec![
            s("Install tmux", "sudo dnf install -y tmux"),
            s("Verify", "tmux -V"),
        ],
        Os::Arch => vec![
            s("Install tmux", "sudo pacman -S --noconfirm tmux"),
            s("Verify", "tmux -V"),
        ],
        Os::Windows => vec![info(
            "tmux doesn't run on native Windows",
            "Use WSL2 and install tmux inside your Linux distro, or try Zellij / Windows Terminal panes instead.",
        )],
        _ => generic_steps("tmux", os),
    }
}

fn rbenv_steps(os: Os) -> Vec<Step> {
    match os {
        Os::MacOs => vec![
            s("Install rbenv and ruby-build", "brew install rbenv ruby-build"),
            sn(
                "Wire rbenv into your shell",
                "echo 'eval \"$(rbenv init - zsh)\"' >> ~/.zshrc",
                "Use `bash` instead of `zsh` if you're on bash. Open a new terminal after.",
            ),
            s("Install a recent Ruby", "rbenv install 3.3.5"),
            s("Make it the default", "rbenv global 3.3.5"),
            s("Verify", "ruby --version"),
        ],
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => vec![
            sn(
                "Install build prerequisites",
                "sudo apt update && sudo apt install -y git curl build-essential libssl-dev libreadline-dev zlib1g-dev libyaml-dev libffi-dev",
                "On Fedora/Arch the package names differ; check your distro's Ruby build-from-source guide.",
            ),
            sn(
                "Clone rbenv",
                "git clone https://github.com/rbenv/rbenv.git ~/.rbenv && \\\n  git clone https://github.com/rbenv/ruby-build.git ~/.rbenv/plugins/ruby-build",
                "Two clones; we install ruby-build as an rbenv plugin so `rbenv install` works.",
            ),
            sn(
                "Wire rbenv into your shell",
                "echo 'export PATH=\"$HOME/.rbenv/bin:$PATH\"' >> ~/.bashrc && \\\n  echo 'eval \"$(rbenv init - bash)\"' >> ~/.bashrc",
                "Use ~/.zshrc and `rbenv init - zsh` if you're on zsh. Open a new terminal after.",
            ),
            s("Install a recent Ruby", "rbenv install 3.3.5"),
            s("Make it the default", "rbenv global 3.3.5"),
            s("Verify", "ruby --version"),
        ],
        Os::Arch => vec![
            s("Install rbenv", "sudo pacman -S --noconfirm rbenv ruby-build"),
            sn(
                "Wire rbenv into your shell",
                "echo 'eval \"$(rbenv init - bash)\"' >> ~/.bashrc",
                "Use ~/.zshrc + `rbenv init - zsh` if you're on zsh.",
            ),
            s("Install a recent Ruby", "rbenv install 3.3.5"),
        ],
        Os::Windows => vec![info(
            "rbenv doesn't have first-class Windows support",
            "On Windows use RubyInstaller (https://rubyinstaller.org) or run rbenv inside WSL2.",
        )],
        _ => generic_steps("rbenv", os),
    }
}

// ── Generic fallback ───────────────────────────────────────────────────────

fn generic_steps(target: &str, os: Os) -> Vec<Step> {
    let pkg = target.trim();
    match os.pkg_install_prefix() {
        Some(cmd) => vec![
            info(
                &format!("I don't have a custom guide for '{pkg}'"),
                "Falling back to your OS package manager. Package names sometimes differ from the project's brand.",
            ),
            sn(
                "Search first to see what the package is called",
                &os.search_command(pkg),
                "If nothing matches, the project may need a different install method (script, archive, repo, etc).",
            ),
            s(&format!("Install '{pkg}'"), &format!("{cmd} {pkg}")),
        ],
        None => vec![info(
            &format!("I couldn't detect a package manager for {}", os.label()),
            "Best path: search the project's website for install instructions.",
        )],
    }
}

// ── Presets ────────────────────────────────────────────────────────────────

/// Curated bundles. Same canonical names as KNOWN_TARGETS, in install order
/// (toolchains before things that depend on them).
const PRESETS: &[(&str, &str, &[&str])] = &[
    (
        "web-dev",
        "Node + Python + git + Docker + Postgres",
        &["git", "node", "python", "docker", "postgres"],
    ),
    (
        "data-sci",
        "Python + git + Postgres + Redis",
        &["git", "python", "postgres", "redis"],
    ),
    (
        "devops",
        "Docker + kubectl + Helm + Terraform + AWS CLI",
        &["docker", "kubectl", "helm", "terraform", "awscli"],
    ),
    (
        "cli-power-user",
        "fzf + ripgrep + bat + jq + tmux + Neovim",
        &["fzf", "ripgrep", "bat", "jq", "tmux", "neovim"],
    ),
    (
        "backend",
        "Go + Postgres + Redis + Docker",
        &["go", "postgres", "redis", "docker"],
    ),
];

fn preset_targets(name: &str) -> Option<&'static [&'static str]> {
    let key = name.trim().to_lowercase();
    PRESETS
        .iter()
        .find(|(n, _, _)| *n == key)
        .map(|(_, _, t)| *t)
}

// ── Fuzzy search & "did you mean?" ─────────────────────────────────────────

/// Tiny bounded Levenshtein. Bails early if distance exceeds `cap`, so
/// callers don't pay full O(n*m) when they only care about close matches.
/// Inputs are lowercased ASCII in practice (target names + aliases).
fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let (n, m) = (av.len(), bv.len());
    if n.abs_diff(m) > cap {
        return cap + 1;
    }
    // Two-row dynamic programming.
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=m {
            let cost = if av[i - 1] == bv[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            if curr[j] < row_min {
                row_min = curr[j];
            }
        }
        if row_min > cap {
            return cap + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// All known names (canonical + aliases + preset names), deduped.
fn all_known_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for (canon, aliases, _) in KNOWN_TARGETS {
        v.push(canon);
        v.extend_from_slice(aliases);
    }
    for (name, _, _) in PRESETS {
        v.push(name);
    }
    v
}

/// Up to `limit` matches sorted by score (best first). Names whose score
/// exceeds `cap` are excluded. Score is `edit_distance - LCP`, with exact
/// substring relationships clamped to 0; this makes 'posg' prefer
/// 'postgres' over short same-distance names like 'pip' or 'go'.
fn fuzzy_matches(query: &str, cap: usize, limit: usize) -> Vec<(&'static str, usize)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(&'static str, usize)> = all_known_names()
        .into_iter()
        .map(|n| {
            let score = if n.contains(&q) || q.contains(n) {
                0
            } else {
                let d = edit_distance(&q, n, cap);
                let lcp = q.bytes().zip(n.bytes()).take_while(|(a, b)| a == b).count();
                d.saturating_sub(lcp)
            };
            (n, score)
        })
        .filter(|(_, s)| *s <= cap)
        .collect();
    scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.len().cmp(&b.0.len())));
    scored.dedup_by(|a, b| a.0 == b.0);
    scored.truncate(limit);
    scored
}

// ── Shell-aware command rewriting ──────────────────────────────────────────

/// Adapt a few command lines for fish, which has different syntax for
/// `source`, `export`, and `eval`-style PATH munging. For other shells we
/// pass commands through unchanged — bash, zsh, sh, and powershell on
/// Windows mostly agree with what the guides already emit.
fn adapt_command_for_shell(cmd: &str, shell: Shell) -> String {
    if shell != Shell::Fish {
        return cmd.to_string();
    }
    let mut out = String::with_capacity(cmd.len());
    for (i, raw_line) in cmd.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // Sourcing the cargo env file: bash uses `. "$HOME/.cargo/env"`,
        // fish has its own file.
        if raw_line.contains(".cargo/env") && raw_line.trim_start().starts_with('.') {
            out.push_str("source \"$HOME/.cargo/env.fish\"");
            continue;
        }
        // `export FOO=bar` → `set -gx FOO bar`. We only touch the simple
        // shape; anything more complex stays as-is with a note in the docs.
        let trimmed = raw_line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("export ") {
            if let Some(eq) = rest.find('=') {
                let name = &rest[..eq];
                let value = &rest[eq + 1..];
                let indent = &raw_line[..raw_line.len() - trimmed.len()];
                out.push_str(&format!("{indent}set -gx {name} {value}"));
                continue;
            }
        }
        out.push_str(raw_line);
    }
    out
}

fn adapt_steps_for_shell(steps: &[Step], shell: Shell) -> Vec<Step> {
    steps
        .iter()
        .map(|st| Step {
            title: st.title.clone(),
            command: st
                .command
                .as_deref()
                .map(|c| adapt_command_for_shell(c, shell)),
            note: st.note.clone(),
        })
        .collect()
}

// ── Uninstall ──────────────────────────────────────────────────────────────

/// Per-target uninstall guides. We keep these terse on purpose — the install
/// guides do the heavy lifting with notes and ordering; removal is usually
/// a one-liner. If a target isn't in this table we fall back to the OS
/// package manager's remove command and the canonical name.
fn uninstall_steps(input: &str, os: Os) -> Vec<Step> {
    let key = canonical(input);
    match (key.as_str(), os) {
        ("docker", Os::MacOs) => vec![s(
            "Uninstall Docker Desktop",
            "brew uninstall --cask docker",
        )],
        ("docker", Os::Ubuntu) | ("docker", Os::Debian) => vec![
            s(
                "Stop the daemon",
                "sudo systemctl disable --now docker || true",
            ),
            s(
                "Remove the Docker packages",
                "sudo apt purge -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
            ),
            sn(
                "Optional: wipe Docker's data",
                "sudo rm -rf /var/lib/docker /var/lib/containerd",
                "This deletes all your images, containers, and volumes. Skip if you want to keep them.",
            ),
        ],
        ("docker", Os::Fedora) => vec![s(
            "Remove Docker",
            "sudo dnf remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
        )],
        ("docker", Os::Arch) => vec![s(
            "Remove Docker",
            "sudo pacman -Rns docker docker-compose",
        )],
        ("nodejs", _) => vec![
            sn(
                "Uninstall the currently active Node version",
                "nvm uninstall $(nvm current)",
                "Repeat with each version listed by `nvm ls` to remove them all.",
            ),
            sn(
                "Remove nvm itself",
                "rm -rf \"$NVM_DIR\" ~/.nvm && \\\n  sed -i.bak '/NVM_DIR/d' ~/.bashrc ~/.zshrc 2>/dev/null || true",
                "Open a new terminal after; the old shell still has nvm's env loaded.",
            ),
        ],
        ("rust", _) => vec![sn(
            "Run rustup's self-uninstaller",
            "rustup self uninstall",
            "Removes ~/.cargo and ~/.rustup. You'll be asked to confirm.",
        )],
        ("homebrew", Os::MacOs) | ("homebrew", Os::LinuxOther) => vec![sn(
            "Run Homebrew's uninstaller",
            "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/uninstall.sh)\"",
            "Official uninstall script. Review it first if you're cautious.",
        )],
        _ => generic_uninstall_steps(input, os),
    }
}

fn generic_uninstall_steps(target: &str, os: Os) -> Vec<Step> {
    let pkg = canonical(target.trim());
    let cmd = match os {
        Os::MacOs => Some(format!("brew uninstall {pkg}")),
        Os::Ubuntu | Os::Debian => Some(format!("sudo apt remove -y {pkg}")),
        Os::Fedora => Some(format!("sudo dnf remove -y {pkg}")),
        Os::Arch => Some(format!("sudo pacman -Rns --noconfirm {pkg}")),
        Os::Windows => Some(format!("winget uninstall -e --id {pkg}")),
        _ => None,
    };
    match cmd {
        Some(c) => vec![
            info(
                &format!("Generic uninstall for '{pkg}'"),
                "Package name may differ from the project's brand; check first with your package manager's search.",
            ),
            s(&format!("Remove '{pkg}'"), &c),
        ],
        None => vec![info(
            &format!("No package manager detected for {}", os.label()),
            "Best path: consult the project's docs for an uninstall procedure.",
        )],
    }
}

// ── Check / detection mode ────────────────────────────────────────────────

/// Most targets put a binary on PATH whose name matches the canonical
/// target. The exceptions live here.
fn check_binary(canon: &str) -> String {
    match canon {
        "nodejs" => "node".to_string(),
        "rust" => "rustc".to_string(),
        "python" => "python3".to_string(),
        "postgresql" => "psql".to_string(),
        "homebrew" => "brew".to_string(),
        "neovim" => "nvim".to_string(),
        "ripgrep" => "rg".to_string(),
        "awscli" => "aws".to_string(),
        "rbenv" => "rbenv".to_string(),
        other => other.to_string(),
    }
}

fn check_steps(input: &str, os: Os) -> Vec<Step> {
    let canon = canonical(input);
    let bin = check_binary(&canon);
    let probe = if matches!(os, Os::Windows) {
        format!("where {bin}")
    } else {
        format!("command -v {bin}")
    };
    vec![
        info(
            &format!("Detect '{canon}' on this system"),
            "If the command prints a path, it's already installed. If not, run `goose` without --check to walk through the install.",
        ),
        sn(
            &format!("Locate the {bin} binary"),
            &probe,
            "Prints the full path, or nothing if it's not on PATH.",
        ),
        sn(
            &format!("Print {bin}'s version"),
            &format!("{bin} --version"),
            "Exact flag varies (some tools use -v or version). If --version fails, try -v.",
        ),
    ]
}

// ── Renderers (text / markdown / json / script) ───────────────────────────

fn render_markdown(guide: &Guide, action: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {action} `{}` on {}\n\n",
        guide.target,
        guide.os.label()
    ));
    out.push_str(&format!("{} step(s).\n\n", guide.steps.len()));
    for (i, st) in guide.steps.iter().enumerate() {
        out.push_str(&format!("## {}. {}\n\n", i + 1, st.title));
        if let Some(cmd) = &st.command {
            out.push_str("```sh\n");
            out.push_str(cmd);
            if !cmd.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if let Some(note) = &st.note {
            out.push_str(&format!("> {note}\n\n"));
        }
    }
    out
}

/// Hand-rolled JSON. We don't pull in serde just to print a few strings.
/// Escapes the four characters that actually matter in our content
/// (backslash, double-quote, newline, tab).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn render_json(guide: &Guide, action: &str) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"action\": \"{}\",\n", json_escape(action)));
    out.push_str(&format!(
        "  \"target\": \"{}\",\n",
        json_escape(&guide.target)
    ));
    out.push_str(&format!(
        "  \"os\": \"{}\",\n",
        json_escape(guide.os.label())
    ));
    out.push_str("  \"steps\": [\n");
    for (i, st) in guide.steps.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"title\": \"{}\",\n",
            json_escape(&st.title)
        ));
        match &st.command {
            Some(c) => out.push_str(&format!("      \"command\": \"{}\",\n", json_escape(c))),
            None => out.push_str("      \"command\": null,\n"),
        }
        match &st.note {
            Some(n) => out.push_str(&format!("      \"note\": \"{}\"\n", json_escape(n))),
            None => out.push_str("      \"note\": null\n"),
        }
        if i + 1 == guide.steps.len() {
            out.push_str("    }\n");
        } else {
            out.push_str("    },\n");
        }
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

/// Reviewable bash script. Crucially, every command is commented out by
/// default — goose's whole philosophy is that *you* decide what runs.
/// The script is meant to be read top-to-bottom, then uncommented
/// piecewise as you go.
fn render_script(guide: &Guide, action: &str) -> String {
    let mut out = String::new();
    out.push_str("#!/usr/bin/env bash\n");
    out.push_str("# Generated by goose. Every command is commented out on purpose:\n");
    out.push_str("# read it, uncomment what you want, run it deliberately.\n");
    out.push_str(&format!(
        "# {action} {} on {}.\n\n",
        guide.target,
        guide.os.label()
    ));
    out.push_str("set -euo pipefail\n\n");
    for (i, st) in guide.steps.iter().enumerate() {
        out.push_str(&format!(
            "# ── step {} of {} — {}\n",
            i + 1,
            guide.steps.len(),
            st.title
        ));
        if let Some(note) = &st.note {
            for line in note.lines() {
                out.push_str(&format!("#   note: {line}\n"));
            }
        }
        if let Some(cmd) = &st.command {
            for line in cmd.lines() {
                out.push_str(&format!("# {line}\n"));
            }
        }
        out.push('\n');
    }
    out
}

// ── UI / walkthrough ───────────────────────────────────────────────────────

fn print_banner() {
    let p = pal();
    println!();
    println!("{}{}  ╭──────╮{}", p.bold, p.cyan, p.reset);
    println!(
        "{}{}  │  ◔   │{}  {}goose{}{} — step-by-step install helper{}",
        p.bold, p.cyan, p.reset, p.bold, p.reset, p.dim, p.reset
    );
    println!(
        "{}{}  ╰──┬───╯{}  {}tells you what to type, never runs it{}",
        p.bold, p.cyan, p.reset, p.dim, p.reset
    );
    println!("{}{}     ╰╮{}", p.bold, p.cyan, p.reset);
    println!();
}

fn read_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let mut line = String::new();
    let n = stdin.lock().read_line(&mut line)?;
    if n == 0 {
        // EOF — treat as quit
        return Ok("q".to_string());
    }
    Ok(line.trim().to_string())
}

fn ask_target() -> io::Result<String> {
    let p = pal();
    println!("{}what are you installing?{}", p.bold, p.reset);
    println!(
        "{}known: docker, node, rust, python, postgres, nginx, go, redis, git,{}",
        p.dim, p.reset
    );
    println!(
        "{}       homebrew, kubectl, gh, terraform{}",
        p.dim, p.reset
    );
    println!(
        "{}anything else falls back to your OS package manager.{}",
        p.dim, p.reset
    );
    println!();
    let answer = read_line(&format!("{}>{}  ", p.magenta, p.reset))?;
    if answer.is_empty() || answer == "q" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no target provided",
        ));
    }
    Ok(answer)
}

fn print_step(i: usize, total: usize, step: &Step) {
    let p = pal();
    println!();
    println!("{}──── step {} of {total} ────{}", p.dim, i + 1, p.reset);
    println!("{}{}{}", p.bold, step.title, p.reset);
    if let Some(cmd) = &step.command {
        println!();
        for (li, line) in cmd.lines().enumerate() {
            if li == 0 {
                println!("  {}$ {}{}", p.cyan, line, p.reset);
            } else {
                println!("    {}{}{}", p.cyan, line, p.reset);
            }
        }
    }
    if let Some(note) = &step.note {
        println!();
        println!(
            "  {}note:{} {}{}{}",
            p.yellow, p.reset, p.dim, note, p.reset
        );
    }
    println!();
}

fn show_all(guide: &Guide) {
    let p = pal();
    println!();
    println!(
        "{}all {} steps for {} on {}{}",
        p.bold,
        guide.steps.len(),
        guide.target,
        guide.os.label(),
        p.reset
    );
    for (i, step) in guide.steps.iter().enumerate() {
        println!();
        println!(
            "{}{}.{} {}{}{}",
            p.dim,
            i + 1,
            p.reset,
            p.bold,
            step.title,
            p.reset
        );
        if let Some(cmd) = &step.command {
            for (li, line) in cmd.lines().enumerate() {
                if li == 0 {
                    println!("   {}$ {}{}", p.cyan, line, p.reset);
                } else {
                    println!("     {}{}{}", p.cyan, line, p.reset);
                }
            }
        }
        if let Some(note) = &step.note {
            println!(
                "   {}note:{} {}{}{}",
                p.yellow, p.reset, p.dim, note, p.reset
            );
        }
    }
    println!();
}

fn walk_through(guide: &Guide) -> io::Result<()> {
    let p = pal();
    let total = guide.steps.len();
    println!();
    println!(
        "{}plan:{} install {}{}{} on {}{}{} — {total} step{}.",
        p.bold,
        p.reset,
        p.green,
        guide.target,
        p.reset,
        p.green,
        guide.os.label(),
        p.reset,
        if total == 1 { "" } else { "s" }
    );
    println!(
        "{}i won't run anything. you copy/paste what makes sense.{}",
        p.dim, p.reset
    );
    println!(
        "{}controls: enter = next  ·  b = back  ·  a = show all  ·  q = quit{}",
        p.dim, p.reset
    );
    println!();
    read_line(&format!("{}press enter to begin >{} ", p.magenta, p.reset))?;

    let mut i: usize = 0;
    while i < total {
        print_step(i, total, &guide.steps[i]);
        let cmd = read_line(&format!("{}>{}  ", p.magenta, p.reset))?;
        match cmd.as_str() {
            "q" | "quit" | "exit" => {
                println!("{}bye.{}", p.dim, p.reset);
                return Ok(());
            }
            "b" | "back" => {
                i = i.saturating_sub(1);
            }
            "a" | "all" | "s" | "show" => {
                show_all(guide);
            }
            _ => {
                i += 1;
            }
        }
    }
    println!();
    println!(
        "{}{}done.{} that's the install. good luck.",
        p.bold, p.green, p.reset
    );
    println!();
    Ok(())
}

fn print_list() {
    let p = pal();
    println!("{}known targets:{}", p.bold, p.reset);
    for (canon, aliases, desc) in KNOWN_TARGETS {
        if aliases.is_empty() {
            println!(
                "  {}{:<11}{} {}{}{}",
                p.cyan, canon, p.reset, p.dim, desc, p.reset
            );
        } else {
            println!(
                "  {}{:<11}{} {}{} (aliases: {}){}",
                p.cyan,
                canon,
                p.reset,
                p.dim,
                desc,
                aliases.join(", "),
                p.reset
            );
        }
    }
    println!();
    println!("{}presets (--preset <name>):{}", p.bold, p.reset);
    for (name, desc, targets) in PRESETS {
        println!(
            "  {}{:<14}{} {}{}{}",
            p.cyan, name, p.reset, p.dim, desc, p.reset
        );
        println!("  {:14}{}→ {}{}", "", p.dim, targets.join(", "), p.reset);
    }
    println!();
    println!(
        "{}anything else falls back to your OS package manager.{}",
        p.dim, p.reset
    );
}

fn print_search_results(query: &str) {
    let p = pal();
    let hits = fuzzy_matches(query, 4, 8);
    if hits.is_empty() {
        println!(
            "{}no known target close to '{}'.{} try `goose --list`.",
            p.dim, query, p.reset
        );
        return;
    }
    println!("{}closest matches for '{}':{}", p.bold, query, p.reset);
    for (name, dist) in hits {
        // Resolve to canonical so users see the real install target.
        let canon = canonical(name);
        let suffix = if canon != name {
            format!(" → {canon}")
        } else {
            String::new()
        };
        let tag = if dist == 0 { "match" } else { "near" };
        println!(
            "  {}{:<14}{} {}{}{}{}",
            p.cyan, name, p.reset, p.dim, tag, suffix, p.reset
        );
    }
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let args = match parse_args(raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("goose: {e}");
            eprintln!("try `goose --help`");
            std::process::exit(2);
        }
    };

    // Decide whether to emit color, then freeze that choice for the rest of
    // the process. Precedence: --no-color > NO_COLOR env > TTY detection.
    // Non-text formats are intended for piping / files, so we strip color
    // unconditionally there.
    let want_color = !args.no_color
        && env::var_os("NO_COLOR").is_none()
        && io::stdout().is_terminal()
        && args.format == OutputFormat::Text;
    let _ = PALETTE.set(if want_color {
        Palette::on()
    } else {
        Palette::off()
    });

    if args.show_help {
        print!("{HELP}");
        return;
    }
    if args.show_version {
        println!("goose {VERSION}");
        return;
    }
    if args.list {
        print_list();
        return;
    }
    if let Some(q) = args.search.as_deref() {
        print_search_results(q);
        return;
    }

    // Resolve targets: explicit positional args take precedence; otherwise
    // a --preset expands into its bundle; otherwise we drop into the
    // interactive prompt.
    let mut targets = args.targets.clone();
    if targets.is_empty() {
        if let Some(name) = args.preset.as_deref() {
            match preset_targets(name) {
                Some(list) => {
                    targets = list.iter().map(|s| s.to_string()).collect();
                }
                None => {
                    eprintln!("goose: unknown preset: {name}");
                    eprintln!("try `goose --list` to see available presets.");
                    std::process::exit(2);
                }
            }
        }
    } else if let Some(name) = args.preset.as_deref() {
        // Mixing both: prepend preset items, dedupe preserving order.
        if let Some(list) = preset_targets(name) {
            let mut combined: Vec<String> = list.iter().map(|s| s.to_string()).collect();
            for t in targets {
                if !combined.iter().any(|c| canonical(c) == canonical(&t)) {
                    combined.push(t);
                }
            }
            targets = combined;
        } else {
            eprintln!("goose: unknown preset: {name}");
            std::process::exit(2);
        }
    }

    // Banner is friendly noise; skip it when piping, when the user gave a
    // target on the CLI, or in any non-text format.
    let interactive = targets.is_empty() && args.format == OutputFormat::Text;
    if interactive {
        print_banner();
    }

    if targets.is_empty() {
        match ask_target() {
            Ok(t) => targets.push(t),
            Err(_) => {
                let p = pal();
                println!("{}nothing to install. bye.{}", p.dim, p.reset);
                std::process::exit(0);
            }
        }
    }

    let os = args.os_override.unwrap_or_else(detect_os);
    let shell = args.shell_override.or_else(Shell::detect);

    // Friendly preflight in text mode only.
    if args.format == OutputFormat::Text && !args.show_all {
        let p = pal();
        println!();
        println!(
            "{}detected:{} {}{}{}",
            p.dim,
            p.reset,
            p.green,
            os.label(),
            p.reset
        );
        if targets.len() > 1 {
            println!(
                "{}queue:{} {}{}{}",
                p.dim,
                p.reset,
                p.green,
                targets.join(" → "),
                p.reset
            );
        }
    }

    // Build action verb for headings: install / uninstall / check.
    let action = if args.uninstall {
        "Uninstall"
    } else if args.check_only {
        "Check"
    } else {
        "Install"
    };

    // Walk every requested target in order. In format=text we go through
    // the interactive walkthrough (or --all dump). In every other format we
    // concatenate rendered output to stdout.
    for (idx, target) in targets.iter().enumerate() {
        // Soft "did you mean?" check: only for unknown canonicals, and only
        // suggest, never block. Keeps the project's "you stay in control"
        // vibe — we just nudge.
        warn_if_typo(target);

        let mut guide = if args.uninstall {
            Guide {
                target: target.trim().to_string(),
                os,
                steps: uninstall_steps(target, os),
            }
        } else if args.check_only {
            Guide {
                target: target.trim().to_string(),
                os,
                steps: check_steps(target, os),
            }
        } else {
            build_guide(target, os)
        };

        if let Some(sh) = shell {
            guide.steps = adapt_steps_for_shell(&guide.steps, sh);
        }

        if guide.steps.is_empty() {
            eprintln!("no guide available for target: {target}");
            continue;
        }

        match args.format {
            OutputFormat::Markdown => {
                if idx > 0 {
                    println!("\n---\n");
                }
                print!("{}", render_markdown(&guide, action));
            }
            OutputFormat::Json => {
                // For multi-target, emit one JSON object per line (JSON Lines).
                // Easier to pipe through `jq` than a single nested array.
                let s = render_json(&guide, action);
                // Compact trailing newline situation.
                print!("{}", s);
            }
            OutputFormat::Script => {
                if idx > 0 {
                    println!();
                    println!("# ════════════════════════════════════════");
                    println!();
                }
                print!("{}", render_script(&guide, action));
            }
            OutputFormat::Text => {
                if args.show_all {
                    show_all(&guide);
                } else if let Err(e) = walk_through(&guide) {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// If the user typed something that doesn't match a known canonical name
/// but is close to one, mention it. Never blocks; the fallback to the OS
/// package manager still runs.
fn warn_if_typo(target: &str) {
    let key = target.trim().to_lowercase();
    // If it's an exact known name (canonical or alias), nothing to warn.
    if KNOWN_TARGETS
        .iter()
        .any(|(c, aliases, _)| *c == key || aliases.contains(&key.as_str()))
    {
        return;
    }
    let hits = fuzzy_matches(&key, 2, 3);
    if hits.is_empty() {
        return;
    }
    let p = pal();
    let names: Vec<String> = hits.iter().map(|(n, _)| (*n).to_string()).collect();
    eprintln!(
        "{}note:{} '{}' isn't a built-in guide. did you mean: {}? falling back to your OS package manager.",
        p.yellow,
        p.reset,
        target,
        names.join(", ")
    );
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ---- canonical / aliases ----

    #[test]
    fn canonical_basic_aliases() {
        assert_eq!(canonical("node"), "nodejs");
        assert_eq!(canonical("npm"), "nodejs");
        assert_eq!(canonical("nvm"), "nodejs");
        assert_eq!(canonical("rust"), "rust");
        assert_eq!(canonical("cargo"), "rust");
        assert_eq!(canonical("rustup"), "rust");
        assert_eq!(canonical("python3"), "python");
        assert_eq!(canonical("pip"), "python");
        assert_eq!(canonical("postgres"), "postgresql");
        assert_eq!(canonical("psql"), "postgresql");
        assert_eq!(canonical("brew"), "homebrew");
        assert_eq!(canonical("golang"), "go");
        assert_eq!(canonical("docker-compose"), "docker");
        assert_eq!(canonical("k8s"), "kubectl");
        assert_eq!(canonical("github-cli"), "gh");
    }

    #[test]
    fn canonical_is_case_and_whitespace_insensitive() {
        assert_eq!(canonical("NPM"), "nodejs");
        assert_eq!(canonical("  Rustup  "), "rust");
        assert_eq!(canonical("\tPostgres\n"), "postgresql");
    }

    #[test]
    fn canonical_unknown_passes_through() {
        assert_eq!(canonical("htop"), "htop");
        assert_eq!(canonical("some-weird-thing"), "some-weird-thing");
    }

    // ---- /etc/os-release parsing ----

    #[test]
    fn parse_os_release_unquoted() {
        let content = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";
        let (id, id_like) = parse_os_release(content);
        assert_eq!(id, "ubuntu");
        assert_eq!(id_like, "debian");
    }

    #[test]
    fn parse_os_release_quoted() {
        let content = "ID=\"rhel\"\nID_LIKE=\"fedora\"\n";
        let (id, id_like) = parse_os_release(content);
        assert_eq!(id, "rhel");
        assert_eq!(id_like, "fedora");
    }

    #[test]
    fn parse_os_release_empty() {
        let (id, id_like) = parse_os_release("");
        assert_eq!(id, "");
        assert_eq!(id_like, "");
    }

    #[test]
    fn parse_os_release_ignores_unrelated_lines() {
        let content = r#"
NAME="Some Distro"
VERSION="1.0 (Codename)"
PRETTY_NAME="Some Distro 1.0"
ID=somedistro
HOME_URL="https://example.com"
"#;
        let (id, id_like) = parse_os_release(content);
        assert_eq!(id, "somedistro");
        assert_eq!(id_like, "");
    }

    // ---- distro classification (including derivatives) ----

    #[test]
    fn classify_ubuntu_direct() {
        assert_eq!(classify_linux("ubuntu", ""), Os::Ubuntu);
    }

    #[test]
    fn classify_pop_is_ubuntu() {
        // Pop!_OS: ID_LIKE="ubuntu debian"
        assert_eq!(classify_linux("pop", "ubuntu debian"), Os::Ubuntu);
    }

    #[test]
    fn classify_mint_is_ubuntu() {
        assert_eq!(classify_linux("linuxmint", "ubuntu"), Os::Ubuntu);
    }

    #[test]
    fn classify_raspbian_is_debian() {
        assert_eq!(classify_linux("raspbian", "debian"), Os::Debian);
    }

    #[test]
    fn classify_endeavouros_is_arch() {
        assert_eq!(classify_linux("endeavouros", "arch"), Os::Arch);
    }

    #[test]
    fn classify_manjaro_is_arch() {
        // Manjaro: ID_LIKE=arch
        assert_eq!(classify_linux("manjaro", "arch"), Os::Arch);
    }

    #[test]
    fn classify_rocky_is_fedora_family() {
        assert_eq!(classify_linux("rocky", "rhel centos fedora"), Os::Fedora);
    }

    #[test]
    fn classify_alma_is_fedora_family() {
        assert_eq!(
            classify_linux("almalinux", "rhel centos fedora"),
            Os::Fedora
        );
    }

    #[test]
    fn classify_unknown_falls_back_to_linux_other() {
        assert_eq!(classify_linux("nixos", ""), Os::LinuxOther);
        assert_eq!(classify_linux("gentoo", ""), Os::LinuxOther);
    }

    // ---- --os parsing ----

    #[test]
    fn parse_os_known_strings() {
        assert_eq!(parse_os("mac"), Some(Os::MacOs));
        assert_eq!(parse_os("MACOS"), Some(Os::MacOs));
        assert_eq!(parse_os("darwin"), Some(Os::MacOs));
        assert_eq!(parse_os("ubuntu"), Some(Os::Ubuntu));
        assert_eq!(parse_os("rocky"), Some(Os::Fedora));
        assert_eq!(parse_os("manjaro"), Some(Os::Arch));
        assert_eq!(parse_os("windows"), Some(Os::Windows));
        assert_eq!(parse_os("win"), Some(Os::Windows));
        assert_eq!(parse_os("linux"), Some(Os::LinuxOther));
    }

    #[test]
    fn parse_os_rejects_garbage() {
        assert_eq!(parse_os("solaris"), None);
        assert_eq!(parse_os(""), None);
    }

    // ---- CLI args ----

    fn args(v: &[&str]) -> Result<Args, String> {
        parse_args(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn cli_no_args_is_ok() {
        let a = args(&[]).unwrap();
        assert_eq!(a, Args::default());
    }

    #[test]
    fn cli_positional_target() {
        let a = args(&["docker"]).unwrap();
        assert_eq!(a.targets, vec!["docker".to_string()]);
        assert!(!a.show_all);
    }

    #[test]
    fn cli_multiple_positional_targets() {
        let a = args(&["docker", "node", "redis"]).unwrap();
        assert_eq!(
            a.targets,
            vec![
                "docker".to_string(),
                "node".to_string(),
                "redis".to_string()
            ]
        );
    }

    #[test]
    fn cli_all_flag_short_and_long() {
        let a = args(&["-a", "docker"]).unwrap();
        assert!(a.show_all);
        assert_eq!(a.targets, vec!["docker".to_string()]);

        let a = args(&["--all", "node"]).unwrap();
        assert!(a.show_all);
        assert_eq!(a.targets, vec!["node".to_string()]);
    }

    #[test]
    fn cli_list_short_and_long() {
        assert!(args(&["-l"]).unwrap().list);
        assert!(args(&["--list"]).unwrap().list);
    }

    #[test]
    fn cli_help_and_version() {
        assert!(args(&["-h"]).unwrap().show_help);
        assert!(args(&["--help"]).unwrap().show_help);
        assert!(args(&["-V"]).unwrap().show_version);
        assert!(args(&["--version"]).unwrap().show_version);
    }

    #[test]
    fn cli_os_override_space_form() {
        let a = args(&["--os", "ubuntu", "docker"]).unwrap();
        assert_eq!(a.os_override, Some(Os::Ubuntu));
        assert_eq!(a.targets, vec!["docker".to_string()]);
    }

    #[test]
    fn cli_os_override_eq_form() {
        let a = args(&["--os=arch", "go"]).unwrap();
        assert_eq!(a.os_override, Some(Os::Arch));
        assert_eq!(a.targets, vec!["go".to_string()]);
    }

    #[test]
    fn cli_os_override_missing_value() {
        assert!(args(&["--os"]).is_err());
    }

    #[test]
    fn cli_os_override_bad_value() {
        assert!(args(&["--os", "haiku"]).is_err());
    }

    #[test]
    fn cli_unknown_long_option() {
        assert!(args(&["--frobnicate"]).is_err());
    }

    #[test]
    fn cli_unknown_short_option() {
        assert!(args(&["-x"]).is_err());
    }

    #[test]
    fn cli_multiple_positional_is_ok() {
        // What used to be an error is now multi-target. Stays as no-error.
        let a = args(&["docker", "rust"]).unwrap();
        assert_eq!(a.targets.len(), 2);
    }

    #[test]
    fn cli_no_color_flag() {
        assert!(args(&["--no-color"]).unwrap().no_color);
    }

    #[test]
    fn cli_double_dash_terminator() {
        // After `--`, dashes are treated as part of the target name.
        let a = args(&["--", "--weird-package-name"]).unwrap();
        assert_eq!(a.targets, vec!["--weird-package-name".to_string()]);
    }

    // ---- new flag parsing ----

    #[test]
    fn cli_format_flag() {
        assert_eq!(
            args(&["--format", "markdown"]).unwrap().format,
            OutputFormat::Markdown
        );
        assert_eq!(args(&["--format=json"]).unwrap().format, OutputFormat::Json);
        assert_eq!(
            args(&["--format", "script"]).unwrap().format,
            OutputFormat::Script
        );
        assert_eq!(args(&["--format=text"]).unwrap().format, OutputFormat::Text);
        assert!(args(&["--format", "yaml"]).is_err());
    }

    #[test]
    fn cli_shell_flag() {
        assert_eq!(
            args(&["--shell", "fish"]).unwrap().shell_override,
            Some(Shell::Fish)
        );
        assert_eq!(
            args(&["--shell=bash"]).unwrap().shell_override,
            Some(Shell::Bash)
        );
        assert!(args(&["--shell", "csh"]).is_err());
    }

    #[test]
    fn cli_uninstall_flag() {
        let a = args(&["--uninstall", "docker"]).unwrap();
        assert!(a.uninstall);
        assert_eq!(a.targets, vec!["docker".to_string()]);
    }

    #[test]
    fn cli_check_flag() {
        let a = args(&["--check", "rust"]).unwrap();
        assert!(a.check_only);
    }

    #[test]
    fn cli_search_flag() {
        let a = args(&["--search", "posg"]).unwrap();
        assert_eq!(a.search.as_deref(), Some("posg"));
        let a = args(&["--search=ngx"]).unwrap();
        assert_eq!(a.search.as_deref(), Some("ngx"));
    }

    #[test]
    fn cli_preset_flag() {
        let a = args(&["--preset", "web-dev"]).unwrap();
        assert_eq!(a.preset.as_deref(), Some("web-dev"));
    }

    // ---- format parsing ----

    #[test]
    fn parse_format_known_strings() {
        assert_eq!(parse_format("md"), Some(OutputFormat::Markdown));
        assert_eq!(parse_format("markdown"), Some(OutputFormat::Markdown));
        assert_eq!(parse_format("JSON"), Some(OutputFormat::Json));
        assert_eq!(parse_format("script"), Some(OutputFormat::Script));
        assert_eq!(parse_format("sh"), Some(OutputFormat::Script));
        assert_eq!(parse_format("text"), Some(OutputFormat::Text));
        assert_eq!(parse_format("plain"), Some(OutputFormat::Text));
        assert_eq!(parse_format("yaml"), None);
    }

    // ---- presets ----

    #[test]
    fn preset_known_names_resolve() {
        assert!(preset_targets("web-dev").is_some());
        assert!(preset_targets("devops").is_some());
        assert!(preset_targets("cli-power-user").is_some());
        assert!(preset_targets("nope").is_none());
    }

    #[test]
    fn preset_items_are_known_targets() {
        // Every item in every preset should be a canonical KNOWN_TARGETS entry,
        // so the preset expansion doesn't accidentally route to the generic fallback.
        for (name, _, items) in PRESETS {
            for item in *items {
                let canon = canonical(item);
                assert!(
                    KNOWN_TARGETS.iter().any(|(c, _, _)| *c == canon),
                    "preset '{name}' references unknown target '{item}'"
                );
            }
        }
    }

    // ---- fuzzy search ----

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("a", "a", 4), 0);
        assert_eq!(edit_distance("kitten", "sitting", 4), 3);
        assert_eq!(edit_distance("", "abc", 4), 3);
        // Bail-out path: very different lengths.
        assert!(edit_distance("a", "abcdefghij", 2) > 2);
    }

    #[test]
    fn fuzzy_matches_finds_close_targets() {
        let hits = fuzzy_matches("posg", 4, 5);
        assert!(
            hits.iter()
                .any(|(n, _)| *n == "postgres" || *n == "postgresql"),
            "expected postgres/postgresql in: {hits:?}"
        );
    }

    #[test]
    fn fuzzy_matches_finds_typo_for_nginx() {
        let hits = fuzzy_matches("ngix", 2, 5);
        assert!(hits.iter().any(|(n, _)| *n == "nginx"));
    }

    #[test]
    fn fuzzy_matches_empty_query_returns_nothing() {
        assert!(fuzzy_matches("", 4, 5).is_empty());
    }

    // ---- shell adaptation ----

    #[test]
    fn shell_adaptation_no_op_for_bash() {
        let cmd = "export PATH=$PATH:/foo";
        assert_eq!(adapt_command_for_shell(cmd, Shell::Bash), cmd);
    }

    #[test]
    fn shell_adaptation_fish_export() {
        let out = adapt_command_for_shell("export FOO=bar", Shell::Fish);
        assert_eq!(out, "set -gx FOO bar");
    }

    #[test]
    fn shell_adaptation_fish_cargo_env() {
        let out = adapt_command_for_shell(". \"$HOME/.cargo/env\"", Shell::Fish);
        assert!(out.contains("source"));
        assert!(out.contains(".cargo/env.fish"));
    }

    // ---- check / uninstall sanity ----

    #[test]
    fn check_steps_have_a_probe() {
        let st = check_steps("docker", Os::MacOs);
        assert!(!st.is_empty());
        assert!(st.iter().any(|s| s
            .command
            .as_deref()
            .is_some_and(|c| c.contains("command -v") || c.contains("where"))));
    }

    #[test]
    fn check_remaps_alias_to_canonical_binary() {
        // 'pip' → python → python3
        let st = check_steps("pip", Os::Ubuntu);
        assert!(st
            .iter()
            .any(|s| s.command.as_deref().is_some_and(|c| c.contains("python3"))));
    }

    #[test]
    fn uninstall_has_steps_for_known_targets() {
        for (canon, _, _) in KNOWN_TARGETS {
            let st = uninstall_steps(canon, Os::Ubuntu);
            assert!(!st.is_empty(), "no uninstall steps for {canon}");
        }
    }

    // ---- renderers ----

    #[test]
    fn render_markdown_has_headers_and_code_block() {
        let guide = build_guide("git", Os::MacOs);
        let md = render_markdown(&guide, "Install");
        assert!(md.starts_with("# Install"));
        assert!(md.contains("```sh"));
        assert!(md.contains("## 1."));
    }

    #[test]
    fn render_json_is_parseable_shape() {
        let guide = build_guide("git", Os::MacOs);
        let j = render_json(&guide, "Install");
        // We don't pull in a JSON parser for tests; just sanity-check
        // that braces balance and the required keys are present.
        assert!(j.starts_with('{'));
        assert!(j.trim_end().ends_with('}'));
        assert!(j.contains("\"target\""));
        assert!(j.contains("\"os\""));
        assert!(j.contains("\"steps\""));
        let opens = j.matches('{').count();
        let closes = j.matches('}').count();
        assert_eq!(opens, closes);
    }

    #[test]
    fn render_json_escapes_quotes_and_newlines() {
        let s = json_escape("a\"b\nc\\d");
        assert_eq!(s, "a\\\"b\\nc\\\\d");
    }

    #[test]
    fn render_script_comments_every_command() {
        let guide = build_guide("git", Os::MacOs);
        let script = render_script(&guide, "Install");
        assert!(script.starts_with("#!/usr/bin/env bash"));
        // Every non-blank line should be a comment or set -e* boilerplate.
        for line in script.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            assert!(
                t.starts_with('#') || t.starts_with("set "),
                "uncommented line in script: {line:?}"
            );
        }
    }

    // ---- new target sanity ----

    #[test]
    fn new_targets_have_steps_on_each_supported_os() {
        let new_targets = [
            "bun", "deno", "pnpm", "neovim", "fzf", "ripgrep", "bat", "jq", "helm", "awscli",
            "tmux", "rbenv",
        ];
        for t in new_targets {
            // At minimum macOS and Ubuntu should produce non-empty steps.
            assert!(
                !build_guide(t, Os::MacOs).steps.is_empty(),
                "no steps for {t} on macOS"
            );
            assert!(
                !build_guide(t, Os::Ubuntu).steps.is_empty(),
                "no steps for {t} on Ubuntu"
            );
        }
    }

    #[test]
    fn new_target_aliases_route_correctly() {
        assert_eq!(canonical("rg"), "ripgrep");
        assert_eq!(canonical("nvim"), "neovim");
        assert_eq!(canonical("vim"), "neovim");
        assert_eq!(canonical("aws"), "awscli");
        assert_eq!(canonical("aws-cli"), "awscli");
        assert_eq!(canonical("ruby"), "rbenv");
    }

    // ---- build_guide sanity ----

    #[test]
    fn build_guide_known_target_has_steps() {
        for (canon, _, _) in KNOWN_TARGETS {
            let guide = build_guide(canon, Os::MacOs);
            assert!(
                !guide.steps.is_empty(),
                "expected steps for {canon} on macOS"
            );
        }
    }

    #[test]
    fn build_guide_unknown_falls_back_to_pkg_manager() {
        let guide = build_guide("htop", Os::Ubuntu);
        assert!(guide.steps.iter().any(|s| s
            .command
            .as_deref()
            .is_some_and(|c| c.contains("apt-cache search"))));
        assert!(guide.steps.iter().any(|s| s
            .command
            .as_deref()
            .is_some_and(|c| c.contains("sudo apt install"))));
    }

    #[test]
    fn build_guide_aliases_route_to_same_guide() {
        let by_alias = build_guide("npm", Os::MacOs).steps.len();
        let by_canon = build_guide("nodejs", Os::MacOs).steps.len();
        assert_eq!(by_alias, by_canon);
    }

    // ---- arch ----

    #[test]
    fn arch_suffix_table() {
        assert_eq!(Arch::X86_64.go_linux_suffix(), "linux-amd64");
        assert_eq!(Arch::Aarch64.go_linux_suffix(), "linux-arm64");
        assert_eq!(Arch::Armv6.go_linux_suffix(), "linux-armv6l");
        assert_eq!(Arch::Other.go_linux_suffix(), "linux-amd64");
    }
}
