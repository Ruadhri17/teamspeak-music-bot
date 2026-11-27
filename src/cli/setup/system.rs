//! The side of `tsmusic setup` that changes the system. Every change goes through `Runner`,
//! which in a dry run only collects the commands to show them.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, bail};
use tempfile::TempDir;

use super::plan::{self, Download, Package, PackageManager, Tool};

/// The last lines of a failed command shown in the error.
const OUTPUT_TAIL_LINES: usize = 15;

pub struct Runner {
    dry_run: bool,
    /// The commands of a dry run since the last `take_shown`.
    shown: Vec<String>,
    /// Downloads and files before they are installed: a new directory with a random name,
    /// created on first use and removed on drop. A name another user could guess, such as
    /// one with the PID, could be prepared by them in `/tmp` with links inside, and setup
    /// runs as root and executes a downloaded binary from here.
    work: Option<TempDir>,
}

impl Runner {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            shown: Vec::new(),
            work: None,
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

    /// A path in the work directory, which is created on first use. A dry run creates
    /// nothing and shows a placeholder for the random part of the name.
    fn work_file(&mut self, name: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        if self.dry_run {
            return Ok(std::env::temp_dir().join("tsmusic-setup-XXXXXX").join(name));
        }
        let dir = match &self.work {
            Some(dir) => dir,
            None => self.work.insert(work_dir()?),
        };
        Ok(dir.path().join(name))
    }
}

/// Created atomically under a random name, owned by the running user with mode `0700`.
/// `tempfile` follows the umask unless told otherwise, so the mode is set explicitly.
fn work_dir() -> anyhow::Result<TempDir> {
    tempfile::Builder::new()
        .prefix("tsmusic-setup-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .context("cannot create a work directory in the temporary directory")
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    #[test]
    fn the_work_directory_is_private_and_new() {
        let mut runner = Runner::new(false);
        let file = runner.work_file("yt-dlp").unwrap();
        let dir = file.parent().unwrap().to_path_buf();
        let meta = fs::metadata(&dir).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);
        assert_eq!(meta.uid(), rustix::process::geteuid().as_raw());
        assert_ne!(
            dir.file_name().unwrap().to_string_lossy(),
            format!("tsmusic-setup-{}", std::process::id())
        );
        assert_eq!(
            runner.work_file("sums").unwrap().parent(),
            Some(dir.as_path())
        );
    }

    #[test]
    fn a_prepared_directory_with_the_old_name_is_not_used() {
        let prepared = std::env::temp_dir().join(format!("tsmusic-setup-{}", std::process::id()));
        fs::create_dir_all(&prepared).unwrap();
        let mut runner = Runner::new(false);
        let file = runner.work_file("yt-dlp").unwrap();
        assert!(!file.starts_with(&prepared));
        fs::remove_dir_all(&prepared).unwrap();
    }

    #[test]
    fn every_runner_gets_its_own_directory() {
        let mut first = Runner::new(false);
        let mut second = Runner::new(false);
        assert_ne!(
            first.work_file("x").unwrap().parent(),
            second.work_file("x").unwrap().parent()
        );
    }

    #[test]
    fn the_work_directory_is_removed_with_the_runner() {
        let mut runner = Runner::new(false);
        let file = runner.work_file("yt-dlp").unwrap();
        fs::write(&file, b"x").unwrap();
        let dir = file.parent().unwrap().to_path_buf();
        drop(runner);
        assert!(!dir.exists());
    }

    #[test]
    fn a_dry_run_creates_nothing() {
        let mut runner = Runner::new(true);
        let file = runner.work_file("yt-dlp").unwrap();
        assert!(file.to_string_lossy().contains("tsmusic-setup-XXXXXX"));
        assert!(!file.parent().unwrap().exists());
    }
}
