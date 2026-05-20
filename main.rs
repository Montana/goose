//! goose — step-by-step install helper.
//!
//! Asks what you're installing, detects your OS (and Linux distro), then walks
//! you through the next commands one at a time. It never runs anything; you
//! copy and paste what you want. The goal is to remember the install dance
//! without giving up control over your machine.

use std::io::{self, BufRead, Write};

// ── ANSI styling ───────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";

// ── OS detection ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let lower = release.to_lowercase();
    if lower.contains("ubuntu") {
        return Os::Ubuntu;
    }
    if lower.contains("debian") {
        return Os::Debian;
    }
    if lower.contains("fedora")
        || lower.contains("rhel")
        || lower.contains("centos")
        || lower.contains("rocky")
        || lower.contains("alma")
    {
        return Os::Fedora;
    }
    if lower.contains("arch") || lower.contains("manjaro") || lower.contains("endeavour") {
        return Os::Arch;
    }
    Os::LinuxOther
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

fn canonical(input: &str) -> String {
    let key = input.trim().to_lowercase();
    let aliases: &[(&str, &str)] = &[
        ("node", "nodejs"),
        ("npm", "nodejs"),
        ("nvm", "nodejs"),
        ("nodejs", "nodejs"),
        ("rust", "rust"),
        ("cargo", "rust"),
        ("rustup", "rust"),
        ("python", "python"),
        ("python3", "python"),
        ("pip", "python"),
        ("pip3", "python"),
        ("docker", "docker"),
        ("docker-compose", "docker"),
        ("git", "git"),
        ("postgres", "postgresql"),
        ("postgresql", "postgresql"),
        ("psql", "postgresql"),
        ("nginx", "nginx"),
        ("go", "go"),
        ("golang", "go"),
        ("redis", "redis"),
        ("brew", "homebrew"),
        ("homebrew", "homebrew"),
    ];
    for (alias, canon) in aliases {
        if key == *alias {
            return (*canon).to_string();
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
        Os::Windows => vec![
            sn(
                "Install Python via winget",
                "winget install -e --id Python.Python.3.12",
                "If you use the GUI installer instead, make sure 'Add Python to PATH' is checked.",
            ),
            s("Verify in a new terminal", "python --version && pip --version"),
        ],
        _ => generic_steps("python", os),
    }
}

fn git_steps(os: Os) -> Vec<Step> {
    let install = match os {
        Os::MacOs => s("Install Git", "brew install git"),
        Os::Ubuntu | Os::Debian => s(
            "Install Git",
            "sudo apt update && sudo apt install -y git",
        ),
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
            s("Install PostgreSQL", "sudo pacman -S --noconfirm postgresql"),
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
        Os::Ubuntu | Os::Debian | Os::Fedora | Os::LinuxOther => vec![
            sn(
                "Download the latest Go tarball",
                "curl -fsSL -O https://go.dev/dl/go1.22.0.linux-amd64.tar.gz",
                "Check https://go.dev/dl for newer versions; the URL changes over time.",
            ),
            s(
                "Replace any existing /usr/local/go install",
                "sudo rm -rf /usr/local/go && sudo tar -C /usr/local -xzf go1.22.0.linux-amd64.tar.gz",
            ),
            sn(
                "Add Go to your PATH",
                "echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.profile",
                "Open a new shell or run 'source ~/.profile' for it to take effect.",
            ),
            s("Verify", "go version"),
        ],
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
    println!();
    println!("{BOLD}{CYAN}  ╭──────╮{RESET}");
    println!(
        "{BOLD}{CYAN}  │  ◔   │{RESET}  {BOLD}goose{RESET}{DIM} — step-by-step install helper{RESET}"
    );
    println!("{BOLD}{CYAN}  ╰──┬───╯{RESET}  {DIM}tells you what to type, never runs it{RESET}");
    println!("{BOLD}{CYAN}     ╰╮{RESET}");
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
    println!("{BOLD}what are you installing?{RESET}");
    println!(
        "{DIM}known: docker, node, rust, python, postgres, nginx, go, redis, git, homebrew{RESET}"
    );
    println!("{DIM}anything else falls back to your OS package manager.{RESET}");
    println!();
    let answer = read_line(&format!("{MAGENTA}>{RESET} "))?;
    if answer.is_empty() || answer == "q" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no target provided",
        ));
    }
    Ok(answer)
}

fn print_step(i: usize, total: usize, step: &Step) {
    println!();
    println!("{DIM}──── step {} of {total} ────{RESET}", i + 1);
    println!("{BOLD}{}{RESET}", step.title);
    if let Some(cmd) = &step.command {
        println!();
        for (li, line) in cmd.lines().enumerate() {
            if li == 0 {
                println!("  {CYAN}$ {}{RESET}", line);
            } else {
                println!("    {CYAN}{}{RESET}", line);
            }
        }
    }
    if let Some(note) = &step.note {
        println!();
        println!("  {YELLOW}note:{RESET} {DIM}{}{RESET}", note);
    }
    println!();
}

fn show_all(guide: &Guide) {
    println!();
    println!(
        "{BOLD}all {} steps for {} on {}{RESET}",
        guide.steps.len(),
        guide.target,
        guide.os.label()
    );
    for (i, step) in guide.steps.iter().enumerate() {
        println!();
        println!("{DIM}{}.{RESET} {BOLD}{}{RESET}", i + 1, step.title);
        if let Some(cmd) = &step.command {
            for (li, line) in cmd.lines().enumerate() {
                if li == 0 {
                    println!("   {CYAN}$ {}{RESET}", line);
                } else {
                    println!("     {CYAN}{}{RESET}", line);
                }
            }
        }
        if let Some(note) = &step.note {
            println!("   {YELLOW}note:{RESET} {DIM}{}{RESET}", note);
        }
    }
    println!();
}

fn walk_through(guide: &Guide) -> io::Result<()> {
    let total = guide.steps.len();
    println!();
    println!(
        "{BOLD}plan:{RESET} install {GREEN}{}{RESET} on {GREEN}{}{RESET} — {total} step{}.",
        guide.target,
        guide.os.label(),
        if total == 1 { "" } else { "s" }
    );
    println!("{DIM}i won't run anything. you copy/paste what makes sense.{RESET}");
    println!("{DIM}controls: enter = next  ·  b = back  ·  a = show all  ·  q = quit{RESET}");
    println!();
    read_line(&format!("{MAGENTA}press enter to begin >{RESET} "))?;

    let mut i: usize = 0;
    while i < total {
        print_step(i, total, &guide.steps[i]);
        let cmd = read_line(&format!("{MAGENTA}>{RESET} "))?;
        match cmd.as_str() {
            "q" | "quit" | "exit" => {
                println!("{DIM}bye.{RESET}");
                return Ok(());
            }
            "b" | "back" => {
                if i > 0 {
                    i -= 1;
                }
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
    println!("{BOLD}{GREEN}done.{RESET} that's the install. good luck.");
    println!();
    Ok(())
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() {
    print_banner();

    let target = match ask_target() {
        Ok(t) => t,
        Err(_) => {
            println!("{DIM}nothing to install. bye.{RESET}");
            std::process::exit(0);
        }
    };

    let os = detect_os();
    println!();
    println!("{DIM}detected:{RESET} {GREEN}{}{RESET}", os.label());

    let guide = build_guide(&target, os);

    if guide.steps.is_empty() {
        eprintln!("no guide available for that target.");
        std::process::exit(1);
    }

    if let Err(e) = walk_through(&guide) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
