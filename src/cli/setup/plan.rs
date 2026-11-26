//! The decisions of `tsmusic setup` without any I/O: where things go, what each program needs,
//! the commands of each package manager and the files setup writes.

/// The bot binary.
pub const BIN: &str = "/usr/local/bin/tsmusic";
/// `yt-dlp` and `deno` from GitHub. Before `/usr/bin` in `PATH`, so it wins over an old
/// copy from the package manager.
pub const TOOLS_DIR: &str = "/usr/local/bin";
/// What the admin writes: the config and the secrets.
pub const CONFIG_DIR: &str = "/etc/tsmusic";
pub const CONFIG_FILE: &str = "/etc/tsmusic/config.toml";
pub const SECRETS_FILE: &str = "/etc/tsmusic/secrets.toml";
/// What the bot writes itself: the identity. Also its home, for the caches of yt-dlp and deno.
pub const STATE_DIR: &str = "/var/lib/tsmusic";
pub const UNIT_FILE: &str = "/etc/systemd/system/tsmusic.service";
/// The service, the user and the group have the same name.
pub const SERVICE: &str = "tsmusic";

/// A program the bot runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    YtDlp,
    Deno,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Ffmpeg, Tool::YtDlp, Tool::Deno];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::YtDlp => "yt-dlp",
            Tool::Deno => "deno",
        }
    }

    pub fn version_flag(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "-version",
            Tool::YtDlp | Tool::Deno => "--version",
        }
    }

    /// What it is for, shown in the list.
    pub fn purpose(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "decodes every track",
            Tool::YtDlp => "YouTube links and searches",
            Tool::Deno => "runs the JavaScript of YouTube for yt-dlp",
        }
    }

    /// Packages its installation needs. `yt-dlp` is a Python zipapp, it does not unpack itself
    /// into `/tmp` like the standalone build. Python also unpacks the zip of `deno`.
    pub fn packages(self) -> &'static [Package] {
        match self {
            Tool::Ffmpeg => &[Package::Ffmpeg],
            Tool::YtDlp | Tool::Deno => &[Package::Python, Package::Curl],
        }
    }

    /// The latest release on GitHub, `None` for a tool from the package manager or an
    /// architecture without a build.
    pub fn download(self, arch: &str) -> Option<Download> {
        match self {
            Tool::Ffmpeg => None,
            Tool::YtDlp => Some(Download {
                url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp".into(),
                sums_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"
                    .into(),
                file: "yt-dlp".into(),
                zipped: false,
            }),
            Tool::Deno => {
                let file = format!("deno-{}.zip", deno_target(arch)?);
                let base = "https://github.com/denoland/deno/releases/latest/download";
                Some(Download {
                    url: format!("{base}/{file}"),
                    sums_url: format!("{base}/{file}.sha256sum"),
                    file,
                    zipped: true,
                })
            }
        }
    }
}

/// A file from a GitHub release and the file with its SHA-256 sum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub url: String,
    pub sums_url: String,
    /// The name in the sums file.
    pub file: String,
    /// A zip with the program inside, under the name of the tool.
    pub zipped: bool,
}

/// The `deno` build for `std::env::consts::ARCH`.
pub fn deno_target(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("x86_64-unknown-linux-gnu"),
        "aarch64" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The sum of `file` from a sums file (`<sha256>  <name>` per line, `*` before the name in
/// binary mode).
pub fn expected_hash<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let hash = words.next()?;
        let name = words.next()?.trim_start_matches('*');
        (name == file).then_some(hash)
    })
}

/// Something setup installs with the package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    Ffmpeg,
    Python,
    Curl,
}

impl Package {
    /// The program that shows the package is there.
    pub fn program(self) -> &'static str {
        match self {
            Package::Ffmpeg => "ffmpeg",
            Package::Python => "python3",
            Package::Curl => "curl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
}

impl PackageManager {
    const ALL: [PackageManager; 3] = [
        PackageManager::Apt,
        PackageManager::Dnf,
        PackageManager::Pacman,
    ];

    /// The first one whose program `exists`.
    pub fn detect(exists: impl Fn(&str) -> bool) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|manager| exists(manager.program()))
    }

    pub fn program(self) -> &'static str {
        match self {
            PackageManager::Apt => "apt-get",
            PackageManager::Dnf => "dnf",
            PackageManager::Pacman => "pacman",
        }
    }

    /// The name of `package` in this distribution. Fedora ships ffmpeg as `ffmpeg-free`.
    pub fn package_name(self, package: Package) -> &'static str {
        match (self, package) {
            (PackageManager::Dnf, Package::Ffmpeg) => "ffmpeg-free",
            (PackageManager::Pacman, Package::Python) => "python",
            (_, Package::Ffmpeg) => "ffmpeg",
            (_, Package::Python) => "python3",
            (_, Package::Curl) => "curl",
        }
    }

    /// The commands that install `packages` without asking. apt refreshes its lists first.
    pub fn install_commands(self, packages: &[Package]) -> Vec<Vec<String>> {
        let names = packages
            .iter()
            .map(|package| self.package_name(*package).to_owned());
        let command = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        match self {
            PackageManager::Apt => vec![
                command(&["apt-get", "update", "-q"]),
                command(&["apt-get", "install", "-y", "-q"])
                    .into_iter()
                    .chain(names)
                    .collect(),
            ],
            PackageManager::Dnf => vec![
                command(&["dnf", "install", "-y"])
                    .into_iter()
                    .chain(names)
                    .collect(),
            ],
            PackageManager::Pacman => vec![
                command(&["pacman", "-S", "--needed", "--noconfirm"])
                    .into_iter()
                    .chain(names)
                    .collect(),
            ],
        }
    }
}

/// The packages `tools` need, each once. A package whose program `exists` is left out,
/// except ffmpeg when it was chosen: then the admin wants it installed or updated.
pub fn packages(tools: &[Tool], exists: impl Fn(&str) -> bool) -> Vec<Package> {
    let mut packages = Vec::new();
    for tool in tools {
        for package in tool.packages() {
            let wanted = *package == Package::Ffmpeg || !exists(package.program());
            if wanted && !packages.contains(package) {
                packages.push(*package);
            }
        }
    }
    packages
}

/// What is chosen at first: the missing tools, or all of them for `--update`.
pub fn preselected(installed: &[(Tool, Option<String>)], update: bool) -> Vec<Tool> {
    installed
        .iter()
        .filter(|(_, version)| update || version.is_none())
        .map(|(tool, _)| *tool)
        .collect()
}

/// The config template with the state in `/var/lib/tsmusic`. The secrets file is relative
/// to the config, so it is `/etc/tsmusic/secrets.toml` already.
pub fn config_file(template: &str) -> String {
    let from = "state_dir = \"state\"";
    assert_eq!(
        template.matches(from).count(),
        1,
        "the template sets state_dir once"
    );
    template.replace(from, &format!("state_dir = \"{STATE_DIR}\""))
}

/// The systemd unit. Restarts after a kick, gives up when every start fails (a ban).
/// The state directory is the only one the bot may write.
pub fn unit_file() -> String {
    format!(
        "\
# Written by `tsmusic setup`, which overwrites it on the next run.
# Change it with `systemctl edit tsmusic` instead.
[Unit]
Description=TeamSpeak music bot
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
User={SERVICE}
Group={SERVICE}
ExecStart={BIN} --config {CONFIG_FILE} run
Restart=on-failure
RestartSec=10s
StateDirectory={SERVICE}
StateDirectoryMode=0700
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
SystemCallArchitectures=native

[Install]
WantedBy=multi-user.target
"
    )
}

#[cfg(test)]
mod tests;
