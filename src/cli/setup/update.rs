//! `tsmusic update`: replaces the bot of the service with a release from GitHub.
//!
//! Finds the release (the latest, or `--version`), downloads the binary for this machine and
//! checks its SHA-256 sum and its version, lets the new binary check the config, then runs
//! the new binary's `setup --update --yes`. The new version knows what it needs itself: its
//! unit, its programs. Until then the old version keeps running, so a failed step changes
//! nothing.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context, bail};
use cliclack::{intro, log, outro, outro_cancel};

use super::{
    plan::{self, Version},
    step,
    system::{self, Runner},
};

/// How long the restarted service gets before it must be running.
const START_GRACE: Duration = Duration::from_secs(5);

/// Clap exits with 2 on an unknown subcommand: a release older than `check-config`.
const USAGE_ERROR: i32 = 2;

/// The flags of `tsmusic update`.
#[derive(Debug, Clone)]
pub struct Options {
    /// A release other than the latest, `0.3.0` or `v0.3.0`.
    pub version: Option<String>,
    /// Install the release even when it is the running version.
    pub force: bool,
}

pub fn run(options: Options) -> anyhow::Result<()> {
    intro(" tsmusic update ")?;
    match steps(&options) {
        Ok(()) => Ok(()),
        Err(err) => {
            outro_cancel(format!("Update failed: {err:#}"))?;
            bail!("update failed")
        }
    }
}

fn steps(options: &Options) -> anyhow::Result<()> {
    if !system::is_root() {
        bail!(
            "update replaces {}, run it as root: sudo tsmusic update",
            plan::BIN
        );
    }
    if !Path::new(plan::UNIT_FILE).exists() {
        bail!(
            "update works on the service of `tsmusic setup`, there is no {}",
            plan::UNIT_FILE
        );
    }
    let mut runner = Runner::new(false);

    let tag = match &options.version {
        Some(version) => plan::release_tag(version)
            .with_context(|| format!("{version:?} is not a version, e.g. 0.3.0"))?,
        None => latest_tag(&mut runner)?,
    };
    let current = env!("CARGO_PKG_VERSION");
    let (running, target) = (
        Version::parse(current).context("the running version cannot be read")?,
        Version::parse(&tag[1..]).context("the tag is not a version")?,
    );
    if !options.force {
        if target == running {
            log::success(format!("tsmusic {current} is up to date."))?;
            outro("Nothing to do.")?;
            return Ok(());
        }
        if target < running && options.version.is_none() {
            log::info(format!(
                "This build ({current}) is newer than the latest release ({tag})."
            ))?;
            outro("Nothing to do, --version installs an older release.")?;
            return Ok(());
        }
    }

    let download = plan::bot_download(&tag, std::env::consts::ARCH)
        .with_context(|| format!("there is no build for {}", std::env::consts::ARCH))?;
    let mut binary = None;
    step(
        &mut runner,
        format!("Downloading tsmusic {tag}"),
        |runner| {
            let file = runner.download(&download)?;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755))
                .context("cannot make the download executable")?;
            binary = Some(file);
            Ok(())
        },
    )?;
    let binary = binary.expect("set by the step");

    step(&mut runner, "Checking the new version", |_| {
        check_version(&binary, &target)
    })?;
    if Path::new(plan::CONFIG_FILE).exists() {
        let mut checked = true;
        step(
            &mut runner,
            format!("Checking {} with the new version", plan::CONFIG_FILE),
            |_| {
                checked = check_config(&binary)?;
                Ok(())
            },
        )?;
        if !checked {
            log::warning("This release cannot check the config, watch the log after the update.")?;
        }
    }

    let was_active = system::service_active();
    outro(format!(
        "tsmusic {current} -> {}: its setup installs it and restarts the service.",
        &tag[1..]
    ))?;
    // The new binary runs from the work directory, which `runner` removes when dropped.
    let status = Command::new(&binary)
        .args(["setup", "--update", "--yes"])
        .stdin(Stdio::null())
        .status()
        .context("cannot start the setup of the new version")?;
    if !status.success() {
        bail!("the setup of tsmusic {tag} failed, see above");
    }
    if was_active {
        thread::sleep(START_GRACE);
        if !system::service_active() {
            bail!(
                "the service does not run after the update, see: journalctl -u {} -n 50",
                plan::SERVICE
            );
        }
    }
    Ok(())
}

/// The tag of the latest release, from where `releases/latest` redirects. No API call, so
/// no rate limit and no JSON.
fn latest_tag(runner: &mut Runner) -> anyhow::Result<String> {
    let url = plan::latest_release_url();
    let effective = runner.output(&[
        "curl",
        "-fsSIL",
        "--proto",
        "=https",
        "--retry",
        "2",
        "-o",
        "/dev/null",
        "-w",
        "%{url_effective}",
        &url,
    ])?;
    plan::tag_from_release_url(&effective)
        .map(str::to_owned)
        .with_context(|| format!("no release found at {url}"))
}

/// The download is the release it claims to be: `tsmusic --version` prints `tsmusic 0.3.0`.
fn check_version(binary: &Path, expected: &Version) -> anyhow::Result<()> {
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .context("the new version does not start")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.split_whitespace().nth(1).and_then(Version::parse);
    if !output.status.success() || version.as_ref() != Some(expected) {
        bail!("the download says it is {:?}", text.trim());
    }
    Ok(())
}

/// A key renamed or removed by the new version would stop the bot after the restart.
/// Only the config: the secrets and the cookies belong to the service user, root cannot
/// pass their owner check. `false` when the release is too old to check it.
fn check_config(binary: &Path) -> anyhow::Result<bool> {
    let output = Command::new(binary)
        .args(["--config", plan::CONFIG_FILE, "check-config"])
        .stdin(Stdio::null())
        .output()
        .context("the new version does not start")?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(USAGE_ERROR) {
        return Ok(false);
    }
    bail!(
        "the new version does not accept the config, nothing was changed. Fix it and run the \
         update again:\n{}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}
