# TeamSpeak music bot

[![CI](https://github.com/Ruadhri17/teamspeak-music-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/Ruadhri17/teamspeak-music-bot/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

An open source music bot for TeamSpeak, written in Rust. It joins a channel as a regular client and plays music from YouTube or from local files, controlled from the channel chat. It is one small binary with no runtime to install, and it keeps the audio steady on a modest VPS.

## Documentation

- [Installing the bot](docs/install.md): the install script, `tsmusic setup`, the systemd service
- [Controlling the bot](docs/commands.md): every chat command, for the people on your server
- [YouTube cookies](docs/youtube.md): when YouTube blocks the server, and for age-restricted videos
- [config.example.toml](config.example.toml): every setting, with its default and what it does
- [Releasing a version](docs/release.md): for maintainers

## Features

What the bot does today:

- **YouTube:** links to videos, Shorts, live streams and YouTube Music, playlists (up to 500 songs by default) and search phrases (`!song add daft punk get lucky`). With the cookies of a YouTube account it also plays age-restricted videos and works on servers that YouTube asks to sign in.
- **Local files:** audio files from a music directory on the bot's server.
- **Queue:** pause, resume, skip, the current song, a paged `!queue show`, removing songs, clearing everything. Searches run in the background and keep the order of the commands.
- **Volume:** a logarithmic scale like most volume controls, heard at once also in the middle of a song, with an optional highest allowed value.
- **Sound quality:** Opus Music at a configurable bitrate, 96 kb/s by default.
- **Channels:** anyone can bring the bot from its home channel by dragging it or with `!bot summon`. While it plays in a channel, only admins can take it elsewhere. It returns home when nothing plays or when it is left alone, and the queue is cleared there.
- **Permissions:** admins and blocked users by TeamSpeak server group, by name or ID.
- **Events:** on connect, disconnect, idle, alone, party (people in the channel) and song start the bot can send a message, play a sound, move, and change its description or nickname, e.g. `Now playing: {song}`.
- **A quiet chat:** the bot answers errors and questions, not every command. It points out typos (`did you mean !song?`) and ignores commands meant for other bots.
- **Checked configuration:** a misspelled key or a wrong value stops the start with the line of the mistake. Passwords live in a separate file that the bot refuses to read when others can read it.
- **Installer:** `tsmusic setup` installs ffmpeg, yt-dlp and deno and sets the bot up as a hardened systemd service. `tsmusic update` moves it to a new release after checking the config with the new version.

Planned:

- More sources: SoundCloud, then Spotify. Tidal once official API access is available.
- Searching for albums and public playlists (`--album`, `--playlist`).
- Settings from the chat (`!bot config list | get | set | reload`) and reloading without a restart.
- Secrets encrypted with systemd credentials.

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/Ruadhri17/teamspeak-music-bot/main/install.sh | sh
```

The script downloads the latest release, checks its SHA-256 sum and starts `tsmusic setup`, which asks what to install and whether to set up the service. Then set the server in `/etc/tsmusic/config.toml`, the passwords in `/etc/tsmusic/secrets.toml`, and start the bot with `sudo systemctl enable --now tsmusic`. The details and the options are in [docs/install.md](docs/install.md).

| | |
| --- | --- |
| **System** | Linux on x86_64 or aarch64, glibc 2.35 or newer |
| **Runtime dependencies** | ffmpeg 6.0 or newer, yt-dlp with Python 3.11 or newer, deno. Setup installs them: ffmpeg and Python from apt, dnf or pacman, so the system's packages must be new enough, yt-dlp and deno from their GitHub releases |
| **Service** | systemd. Without it the bot runs in the foreground with `tsmusic --config <file> run` |
| **TeamSpeak server** | TeamSpeak 3, people may use TeamSpeak 3 or 6 clients. The bot's server group needs `b_client_modify_own_description` to set its description, and should be permanent (`b_group_is_permanent`), or the bot loses it on every reconnect |
| **Sources** | YouTube and local files |

## Contributing

Contributions are welcome:

1. Fork the repository and create a branch.
2. Run `just deps` once, it installs what building and testing need.
3. Make the change, with tests where it makes sense, and run `just check` (format, lints, tests, cargo-deny).
4. Open a pull request to `main`. The pull request template lists what to check.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:` ...). Using AI tools is fine, as long as you understand the change and have tested it yourself.

Ideas for new features and bug reports go to [issues](https://github.com/Ruadhri17/teamspeak-music-bot/issues/new/choose), each has its own template. Security problems are reported privately instead, see [SECURITY.md](SECURITY.md).

## Acknowledgements

Thanks to the [ReSpeak](https://github.com/ReSpeak) team for [tsclientlib](https://github.com/ReSpeak/tsclientlib), the TeamSpeak client library in Rust that the bot connects with. Without it the bot would not exist.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
