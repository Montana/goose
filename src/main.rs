use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::OnceLock;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
goose — step-by-step install helper

USAGE:
    goose [OPTIONS] [TARGET]

ARGUMENTS:
    [TARGET]    Thing to install (e.g. docker, node, rust).
                If omitted, goose asks interactively.

OPTIONS:
    -a, --all          Print every step at once (skip the walkthrough)
    -l, --list         List known targets and exit
        --os <NAME>    Override OS detection. One of:
                       macos, ubuntu, debian, fedora, arch, linux, windows
        --no-color     Disable ANSI color (also: set NO_COLOR=1)
    -h, --help         Show this message and exit
    -V, --version      Show version and exit

EXAMPLES:
    goose docker
    goose --all postgres
    goose --os ubuntu kubectl
    goose --list

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

// ── CLI args ───────────────────────────────────────────────────────────────

#[derive(Default, Debug, PartialEq, Eq)]
struct Args {
    target: Option<String>,
    show_all: bool,
    list: bool,
    os_override: Option<Os>,
    no_color: bool,
    show_help: bool,
    show_version: bool,
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
            "--" => {
                // Everything after `--` is positional.
                for rest in iter.by_ref() {
                    if out.target.is_some() {
                        return Err(format!("unexpected extra argument: {rest}"));
                    }
                    out.target = Some(rest);
                }
            }
            s if s.starts_with("--") => {
                return Err(format!("unknown option: {s}"));
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!("unknown option: {s}"));
            }
            _ => {
                if out.target.is_some() {
                    return Err(format!("unexpected extra argument: {a}"));
                }
                out.target = Some(a);
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
    println!(
        "{}anything else falls back to your OS package manager.{}",
        p.dim, p.reset
    );
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
    let want_color =
        !args.no_color && env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
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

    // Banner is friendly noise; skip it when piping a one-shot --all or when
    // the user gave a target on the CLI (they already know what they want).
    if args.target.is_none() {
        print_banner();
    }

    let target = match args.target.clone() {
        Some(t) => t,
        None => match ask_target() {
            Ok(t) => t,
            Err(_) => {
                let p = pal();
                println!("{}nothing to install. bye.{}", p.dim, p.reset);
                std::process::exit(0);
            }
        },
    };

    let os = args.os_override.unwrap_or_else(detect_os);
    let p = pal();
    if !args.show_all {
        println!();
        println!(
            "{}detected:{} {}{}{}",
            p.dim,
            p.reset,
            p.green,
            os.label(),
            p.reset
        );
    }

    let guide = build_guide(&target, os);

    if guide.steps.is_empty() {
        eprintln!("no guide available for that target.");
        std::process::exit(1);
    }

    if args.show_all {
        show_all(&guide);
        return;
    }

    if let Err(e) = walk_through(&guide) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
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
        assert_eq!(a.target.as_deref(), Some("docker"));
        assert!(!a.show_all);
    }

    #[test]
    fn cli_all_flag_short_and_long() {
        let a = args(&["-a", "docker"]).unwrap();
        assert!(a.show_all);
        assert_eq!(a.target.as_deref(), Some("docker"));

        let a = args(&["--all", "node"]).unwrap();
        assert!(a.show_all);
        assert_eq!(a.target.as_deref(), Some("node"));
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
        assert_eq!(a.target.as_deref(), Some("docker"));
    }

    #[test]
    fn cli_os_override_eq_form() {
        let a = args(&["--os=arch", "go"]).unwrap();
        assert_eq!(a.os_override, Some(Os::Arch));
        assert_eq!(a.target.as_deref(), Some("go"));
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
    fn cli_too_many_positional() {
        assert!(args(&["docker", "rust"]).is_err());
    }

    #[test]
    fn cli_no_color_flag() {
        assert!(args(&["--no-color"]).unwrap().no_color);
    }

    #[test]
    fn cli_double_dash_terminator() {
        // After `--`, dashes are treated as part of the target name.
        let a = args(&["--", "--weird-package-name"]).unwrap();
        assert_eq!(a.target.as_deref(), Some("--weird-package-name"));
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
