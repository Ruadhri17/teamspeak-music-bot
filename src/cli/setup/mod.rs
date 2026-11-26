//! `tsmusic setup`: installs what the bot needs and sets it up as a systemd service.
//!
//! Asks which programs to install (the missing ones are chosen already), then whether to set
//! up the service: the `tsmusic` user, `/etc/tsmusic` with templates of the config and the
//! secrets, `/var/lib/tsmusic`, the binary in `/usr/local/bin` and the unit. Existing config
//! and secrets are never overwritten. `--yes` takes the defaults without asking, `--dry-run`
//! only shows the commands.
//!
//! - `mod.rs`: the questions and the order of the steps
//! - `plan`: what to install and how, the files setup writes, without I/O
//! - `system`: running the steps

mod plan;
mod system;

use std::{
    fmt::Display,
    io::{self, IsTerminal},
    path::Path,
};

use anyhow::{Context, bail};
use cliclack::{confirm, intro, log, multiselect, note, outro, outro_cancel, spinner};

use crate::config::TEMPLATE;
use plan::{PackageManager, Tool};
use system::Runner;

/// Written as `/etc/tsmusic/secrets.toml` when there is none.
const SECRETS_TEMPLATE: &str = include_str!("../../../secrets.example.toml");

/// The flags of `tsmusic setup`.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Take the defaults without asking.
    pub yes: bool,
    /// Download `yt-dlp` and `deno` again and restart a running service.
    pub update: bool,
    /// Change nothing, show the commands.
    pub dry_run: bool,
}

pub fn run(options: Options) -> anyhow::Result<()> {
    if !options.yes && !io::stdin().is_terminal() {
        bail!("setup asks questions and needs a terminal, use --yes to take the defaults");
    }
    intro(" tsmusic setup ")?;
    let result = steps(options);
    match &result {
        Ok(()) => {}
        Err(err) if is_cancel(err) => outro_cancel("Cancelled, nothing else was changed.")?,
        Err(err) => outro_cancel(format!("Setup failed: {err:#}"))?,
    }
    // The error is already on the screen.
    result.or_else(|err| {
        if is_cancel(&err) {
            Ok(())
        } else {
            bail!("setup failed")
        }
    })
}

fn steps(options: Options) -> anyhow::Result<()> {
    let mut runner = Runner::new(options.dry_run);
    if options.dry_run {
        log::info("Dry run: nothing is changed, the commands are only shown.")?;
    } else if !system::is_root() {
        bail!("setup installs packages and creates a user, run it as root: sudo tsmusic setup");
    }

    let manager = PackageManager::detect(system::in_path);
    let installed: Vec<(Tool, Option<String>)> = Tool::ALL
        .iter()
        .map(|tool| (*tool, system::installed_version(*tool)))
        .collect();
    let tools = choose_tools(&installed, options)?;
    warn_missing(&installed, &tools)?;

    let service = if !system::has_systemd() {
        log::warning("systemd is not running here, the service is skipped.")?;
        false
    } else {
        note("The service", service_summary())?;
        options.yes
            || confirm("Set up the tsmusic service?")
                .initial_value(true)
                .interact()?
    };

    install_tools(&mut runner, manager, &tools)?;
    let restarted = if service {
        set_up_service(&mut runner, options.update)?
    } else {
        false
    };
    finish(service, restarted, options.dry_run)
}

/// The multi-select of programs, each with its version or "missing".
fn choose_tools(
    installed: &[(Tool, Option<String>)],
    options: Options,
) -> anyhow::Result<Vec<Tool>> {
    let preselected = plan::preselected(installed, options.update);
    if options.yes {
        return Ok(preselected);
    }
    let mut prompt = multiselect("Which programs should be installed or updated?")
        .initial_values(preselected)
        .required(false);
    for (tool, version) in installed {
        let hint = match version {
            Some(version) => format!("{version} installed, {}", tool.purpose()),
            None => format!("missing, {}", tool.purpose()),
        };
        prompt = prompt.item(*tool, tool.name(), hint);
    }
    Ok(prompt.interact()?)
}

/// The bot checks its programs at start and refuses to run without them.
fn warn_missing(installed: &[(Tool, Option<String>)], chosen: &[Tool]) -> io::Result<()> {
    let missing: Vec<&str> = installed
        .iter()
        .filter(|(tool, version)| version.is_none() && !chosen.contains(tool))
        .map(|(tool, _)| tool.name())
        .collect();
    if !missing.is_empty() {
        log::warning(format!(
            "The bot does not start without {}, install it before starting the service.",
            missing.join(", ")
        ))?;
    }
    Ok(())
}

fn service_summary() -> String {
    format!(
        "user and group     {service}\n\
         config, secrets    {config_dir} (existing files are kept)\n\
         state              {state}\n\
         binary             {bin}\n\
         unit               {unit}",
        service = plan::SERVICE,
        config_dir = plan::CONFIG_DIR,
        state = plan::STATE_DIR,
        bin = plan::BIN,
        unit = plan::UNIT_FILE,
    )
}

fn install_tools(
    runner: &mut Runner,
    manager: Option<PackageManager>,
    tools: &[Tool],
) -> anyhow::Result<()> {
    if tools.is_empty() {
        return Ok(());
    }
    let packages = plan::packages(tools, system::in_path);
    if !packages.is_empty() {
        let Some(manager) = manager else {
            bail!(
                "no apt-get, dnf or pacman found, install {} yourself",
                packages
                    .iter()
                    .map(|package| package.program())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        let names: Vec<&str> = packages
            .iter()
            .map(|package| manager.package_name(*package))
            .collect();
        step(
            runner,
            format!("Installing {} with {}", names.join(", "), manager.program()),
            |runner| system::install_packages(runner, manager, &packages),
        )?;
    }
    for tool in tools {
        let Some(download) = tool.download(std::env::consts::ARCH) else {
            if *tool != Tool::Ffmpeg {
                log::warning(format!(
                    "{} has no build for {}, install it yourself",
                    tool.name(),
                    std::env::consts::ARCH
                ))?;
            }
            continue;
        };
        step(
            runner,
            format!("Installing {} from GitHub", tool.name()),
            |runner| runner.install_download(*tool, &download),
        )?;
    }
    Ok(())
}

/// Returns whether the running service was restarted.
fn set_up_service(runner: &mut Runner, update: bool) -> anyhow::Result<bool> {
    let service = plan::SERVICE;
    if !system::user_exists(service) {
        step(runner, format!("Creating the user {service}"), |runner| {
            runner.run(&[
                "useradd",
                "--system",
                "--user-group",
                "--home-dir",
                plan::STATE_DIR,
                "--no-create-home",
                "--shell",
                system::nologin_shell(),
                service,
            ])
        })?;
    }
    step(runner, "Creating the directories", |runner| {
        runner.run(&[
            "install",
            "-d",
            "-m",
            "0750",
            "-o",
            "root",
            "-g",
            service,
            plan::CONFIG_DIR,
        ])?;
        runner.run(&[
            "install",
            "-d",
            "-m",
            "0700",
            "-o",
            service,
            "-g",
            service,
            plan::STATE_DIR,
        ])
    })?;
    if !Path::new(plan::CONFIG_FILE).exists() {
        step(runner, format!("Writing {}", plan::CONFIG_FILE), |runner| {
            runner.write_file(
                &plan::config_file(TEMPLATE),
                plan::CONFIG_FILE,
                "0640",
                ("root", service),
            )
        })?;
    }
    if !Path::new(plan::SECRETS_FILE).exists() {
        // The bot refuses a secrets file that is not its own or that others can read.
        step(
            runner,
            format!("Writing {}", plan::SECRETS_FILE),
            |runner| {
                runner.write_file(
                    SECRETS_TEMPLATE,
                    plan::SECRETS_FILE,
                    "0600",
                    (service, service),
                )
            },
        )?;
    }
    let mut binary_changed = false;
    step(
        runner,
        format!("Installing the binary as {}", plan::BIN),
        |runner| {
            binary_changed = system::install_binary(runner)?;
            Ok(())
        },
    )?;
    step(runner, format!("Writing {}", plan::UNIT_FILE), |runner| {
        runner.write_file(
            &plan::unit_file(),
            plan::UNIT_FILE,
            "0644",
            ("root", "root"),
        )?;
        runner.run(&["systemctl", "daemon-reload"])
    })?;
    if (update || binary_changed) && !runner.dry_run() && system::service_active() {
        step(runner, "Restarting the service", |runner| {
            runner.run(&["systemctl", "restart", service])
        })?;
        return Ok(true);
    }
    Ok(false)
}

fn finish(service: bool, restarted: bool, dry_run: bool) -> anyhow::Result<()> {
    if service && !restarted {
        note(
            "Next steps",
            format!(
                "1. Set the server in {config}: address, nickname, home_channel\n\
                 2. Put the passwords in {secrets}\n\
                 3. sudo systemctl enable --now {service}\n\
                 4. journalctl -u {service} -f   shows the log",
                config = plan::CONFIG_FILE,
                secrets = plan::SECRETS_FILE,
                service = plan::SERVICE,
            ),
        )?;
    }
    if restarted {
        log::success("The service runs the new version.")?;
    }
    outro(if dry_run {
        "Dry run finished, nothing was changed."
    } else {
        "Setup finished."
    })?;
    Ok(())
}

/// Runs one step with a spinner, the dry-run commands are listed under it.
fn step(
    runner: &mut Runner,
    title: impl Display,
    action: impl FnOnce(&mut Runner) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let title = title.to_string();
    let progress = spinner();
    progress.start(&title);
    match action(runner) {
        Ok(()) => {
            // Cleared and logged, so the title stays also where no spinner is drawn.
            progress.clear();
            log::step(&title)?;
            let shown = runner.take_shown();
            if !shown.is_empty() {
                log::remark(
                    shown
                        .iter()
                        .map(|command| format!("$ {command}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )?;
            }
            Ok(())
        }
        Err(err) => {
            progress.clear();
            log::error(&title)?;
            Err(err).context(title)
        }
    }
}

/// Escape or Ctrl-C in a prompt.
fn is_cancel(err: &anyhow::Error) -> bool {
    err.downcast_ref::<io::Error>()
        .is_some_and(|err| err.kind() == io::ErrorKind::Interrupted)
}
