//! The side of `tsmusic setup` that changes the system. Every change goes through `Runner`,
//! which in a dry run only collects the commands to show them.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, bail};

use super::plan::{self, Download, Package, PackageManager, Tool};

/// The last lines of a failed command shown in the error.
const OUTPUT_TAIL_LINES: usize = 15;

pub struct Runner {
    dry_run: bool,
    /// The commands of a dry run since the last `take_shown`.
    shown: Vec<String>,
    /// Downloads and files before they are installed, removed on drop.
    work: PathBuf,
}

impl Runner {
    pub fn new(dry_run: bool) -> Self {
        let work = std::env::temp_dir().join(format!("tsmusic-setup-{}", std::process::id()));
        Self {
            dry_run,
            shown: Vec::new(),
            work,
        }
    }

    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// The commands a dry run would have run, since the last call.
    pub fn take_shown(&mut self) -> Vec<String> {
        std::mem::take(&mut self.shown)
    }

    /// Runs `args` and waits. The output is kept for the error only, so it does not break
    /// the spinner. apt asks nothing with `DEBIAN_FRONTEND`, the others ignore it.
    pub fn run(&mut self, args: &[&str]) -> anyhow::Result<()> {
        if self.dry_run {
            self.shown.push(args.join(" "));
            return Ok(());
        }
        self.output(args).map(drop)
    }

    /// Runs `args`, waits and returns what it printed. Also in a dry run, only for commands
    /// that change nothing.
    pub fn output(&mut self, args: &[&str]) -> anyhow::Result<String> {
        let output = Command::new(args[0])
            .args(&args[1..])
            .env("DEBIAN_FRONTEND", "noninteractive")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot start {}", args[0]))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let text = String::from_utf8_lossy(&output.stderr).into_owned()
            + &String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(OUTPUT_TAIL_LINES)..].join("\n");
        bail!("`{}` failed ({}):\n{tail}", args.join(" "), output.status)
    }

    /// Installs `contents` as `dest` with `mode` and the owner `user:group`.
    pub fn write_file(
        &mut self,
        contents: &str,
        dest: &str,
        mode: &str,
        owner: (&str, &str),
    ) -> anyhow::Result<()> {
        let temp = self.work_file(Path::new(dest).file_name().unwrap_or_default())?;
        if !self.dry_run {
            fs::write(&temp, contents)
                .with_context(|| format!("cannot write {}", temp.display()))?;
        }
        install_file(self, &temp, dest, mode, owner)
    }

    /// Downloads a file from a GitHub release into the work directory and checks its sum.
    pub fn download(&mut self, download: &Download) -> anyhow::Result<PathBuf> {
        let file = self.work_file(&download.file)?;
        let sums = self.work_file(format!("{}.sums", download.file))?;
        self.fetch(&download.url, &file)?;
        self.fetch(&download.sums_url, &sums)?;
        if !self.dry_run {
            verify(&file, &sums, &download.file)?;
        }
        Ok(file)
    }

    /// Installs `tool` from its GitHub release into `TOOLS_DIR`, after checking its sum.
    pub fn install_download(&mut self, tool: Tool, download: &Download) -> anyhow::Result<()> {
        let file = self.download(download)?;
        let program = if download.zipped {
            let dir = self.work_file(format!("{}-unzipped", tool.name()))?;
            let (zip, dir_text) = (file.display().to_string(), dir.display().to_string());
            self.run(&["python3", "-m", "zipfile", "-e", &zip, &dir_text])?;
            dir.join(tool.name())
        } else {
            file
        };
        let dest = format!("{}/{}", plan::TOOLS_DIR, tool.name());
        install_file(self, &program, &dest, "0755", ("root", "root"))
    }

    /// Downloads `url` to `dest` over HTTPS only.
    fn fetch(&mut self, url: &str, dest: &Path) -> anyhow::Result<()> {
        let dest = dest.display().to_string();
        self.run(&[
            "curl", "-fsSL", "--proto", "=https", "--retry", "2", "-o", &dest, url,
        ])
    }

    /// A path in the work directory, which is created on first use.
    fn work_file(&mut self, name: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        if !self.dry_run {
            fs::create_dir_all(&self.work)
                .with_context(|| format!("cannot create {}", self.work.display()))?;
            fs::set_permissions(&self.work, fs::Permissions::from_mode(0o700))?;
        }
        Ok(self.work.join(name))
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        // Only temporary copies, a leftover in /tmp is harmless.
        let _ = fs::remove_dir_all(&self.work);
    }
}

fn install_file(
    runner: &mut Runner,
    source: &Path,
    dest: &str,
    mode: &str,
    (user, group): (&str, &str),
) -> anyhow::Result<()> {
    let source = source.display().to_string();
    runner.run(&[
        "install", "-m", mode, "-o", user, "-g", group, &source, dest,
    ])
}

/// Compares the SHA-256 of `file` with its line in `sums`.
fn verify(file: &Path, sums: &Path, name: &str) -> anyhow::Result<()> {
    let sums = fs::read_to_string(sums).context("cannot read the checksums")?;
    let expected = plan::expected_hash(&sums, name)
        .with_context(|| format!("the checksums do not list {name}"))?;
    let output = Command::new("sha256sum")
        .arg(file)
        .output()
        .context("cannot start sha256sum")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let actual = text.split_whitespace().next().unwrap_or_default();
    if !output.status.success() || !actual.eq_ignore_ascii_case(expected) {
        bail!("the checksum of {name} does not match, the download was not installed");
    }
    Ok(())
}

/// A program by its name in `PATH`.
pub fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            fs::metadata(dir.join(program))
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
    })
}

/// The version `tool` prints, `None` when it does not run.
pub fn installed_version(tool: Tool) -> Option<String> {
    let output = Command::new(tool.name())
        .arg(tool.version_flag())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or_default();
    Some(crate::version_number(first_line).to_owned())
}

pub fn is_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// systemd runs as the init system, not only installed.
pub fn has_systemd() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

pub fn user_exists(name: &str) -> bool {
    Command::new("id")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", plan::SERVICE])
        .status()
        .is_ok_and(|status| status.success())
}

/// The shell of a system user that cannot log in, where this distribution keeps it.
pub fn nologin_shell() -> &'static str {
    ["/usr/sbin/nologin", "/sbin/nologin", "/usr/bin/nologin"]
        .into_iter()
        .find(|path| Path::new(path).exists())
        .unwrap_or("/bin/false")
}

/// Installs `packages` with `manager`.
pub fn install_packages(
    runner: &mut Runner,
    manager: PackageManager,
    packages: &[Package],
) -> anyhow::Result<()> {
    for command in manager.install_commands(packages) {
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        runner.run(&args)?;
    }
    Ok(())
}

/// Copies the running binary to `BIN`, unless it already runs from there.
pub fn install_binary(runner: &mut Runner) -> anyhow::Result<bool> {
    let current = std::env::current_exe().context("cannot find the running binary")?;
    if current == Path::new(plan::BIN) {
        return Ok(false);
    }
    install_file(runner, &current, plan::BIN, "0755", ("root", "root"))?;
    Ok(true)
}
