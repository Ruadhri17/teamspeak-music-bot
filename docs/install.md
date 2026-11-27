# Installing the music bot

For the administrator of the server the bot runs on. The bot runs on Linux, on x86_64 or aarch64.

| Runtime dependency | Version | Check with |
| --- | --- | --- |
| glibc | 2.35 or newer | `ldd --version` |
| ffmpeg | 6.0 or newer, for local files and event sounds | `ffmpeg -version` |
| Python | 3.11 or newer, for yt-dlp | `python3 --version` |

Setup installs ffmpeg and Python from the package manager, so the versions of your system's packages count.

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/Ruadhri17/teamspeak-music-bot/main/install.sh | sh
```

The script downloads the bot from the latest release, checks its SHA-256 sum and starts `tsmusic setup` as root (through `sudo`). Setup asks two things:

1. **Which programs to install or update.** Missing ones are already selected:
   - `ffmpeg`, from your package manager (apt, dnf or pacman). It decodes every track.
   - `yt-dlp`, the latest release from GitHub. It finds and streams YouTube.
   - `deno`, the latest release from GitHub. yt-dlp needs it to run the JavaScript of YouTube.

   Downloads from GitHub are checked against their published SHA-256 sums. The bot does not start without these three programs.
2. **Whether to set up the service.** This creates:

   | What | Where |
   | --- | --- |
   | User and group | `tsmusic` |
   | Config and secrets | `/etc/tsmusic/config.toml`, `/etc/tsmusic/secrets.toml` (existing files are kept) |
   | The bot's own files (identity) | `/var/lib/tsmusic` |
   | The binary | `/usr/local/bin/tsmusic` |
   | The systemd unit | `/etc/systemd/system/tsmusic.service` |

Then:

1. Set the server in `/etc/tsmusic/config.toml`: `address`, `nickname`, `home_channel`.
2. Put the server and channel passwords in `/etc/tsmusic/secrets.toml`. The file must stay owned by `tsmusic` with mode `0600`, otherwise the bot refuses to start.
3. Start the bot and enable it at boot: `sudo systemctl enable --now tsmusic`
4. Follow the log: `journalctl -u tsmusic -f`

## Options

Options of `install.sh` go to `tsmusic setup`, for example `curl ... | sh -s -- --yes`.

| Option | What it does |
| --- | --- |
| `--yes` | Takes the defaults without asking: installs what is missing and sets up the service. |
| `--update` | Downloads yt-dlp and deno again and restarts a running service. With the script it also installs the latest bot, see [Updating](#updating). |
| `--dry-run` | Changes nothing, only shows the commands setup would run. |
| `--tools-only` | Installs the programs and skips the service, e.g. when you run the bot yourself. |

`TSMUSIC_VERSION=v0.2.0` before `sh` installs a release other than the latest.

## Updating

```sh
sudo tsmusic update
```

It finds the latest release and does nothing when the bot is up to date. Otherwise it downloads the bot for this machine, checks its SHA-256 sum and lets the new version check `/etc/tsmusic/config.toml`. If a setting was renamed or removed, the update stops with the line to fix and the running bot is left as it is. Then the new version runs its own setup: it installs itself, downloads yt-dlp and deno again, rewrites the unit and restarts the service. At the end the update checks that the service is running.

| Option | What it does |
| --- | --- |
| `--version 0.3.0` | Installs this release instead of the latest, also an older one. |
| `--force` | Installs the release even when it is the running version, e.g. to repair an installation. |

Versions before 0.3.0 have no `tsmusic update`. Update them once with the install script and `--update`.

To check the config by hand after editing it: `tsmusic --config /etc/tsmusic/config.toml check-config`.

## Without the script

Download `tsmusic-<architecture>-unknown-linux-gnu` and `SHA256SUMS` from the release, check the sum with `sha256sum -c --ignore-missing SHA256SUMS`, then run `sudo ./tsmusic-<architecture>-unknown-linux-gnu setup`.

## Notes

- If `tsmusic yt-check` fails with `Sign in to confirm you're not a bot`, YouTube blocks the server. Give the bot the cookies of a YouTube account, see [YouTube cookies](youtube.md).
- The service may write only to `/var/lib/tsmusic`. It cannot read home directories, so keep `sources.local.music_dir` elsewhere, e.g. in `/srv/music`.
- Running setup again is safe. It installs what you select and rewrites the unit, but keeps the config and the secrets. Change the unit with `sudo systemctl edit tsmusic`, setup overwrites the file itself.
