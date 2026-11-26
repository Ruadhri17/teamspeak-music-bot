# Security policy

## Supported versions

Only the latest release gets security fixes. Update with `install.sh --update` (see [docs/install.md](docs/install.md)) before you report, the problem may be fixed already.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through the [security advisory form](https://github.com/Ruadhri17/teamspeak-music-bot/security/advisories/new), only the maintainer sees it there.

Include:

- the bot version (`tsmusic --version`) and how it is installed,
- what an attacker can do and what they need for it (a chat message, access to the server, a local account ...),
- the steps that show it, e.g. the chat commands or a link,
- a fix or an idea for one, if you have it.

Leave out real passwords, tokens and cookie files, also your own.

## What happens next

The project is maintained in spare time. You get an answer within a week, usually sooner. Once the problem is confirmed, a fix comes in a new release, and the advisory is published with it. You are credited in the advisory, unless you prefer not to be.

## Scope

In scope is everything in this repository, for example:

- chat commands that get around the permissions, or make the bot run something it should not,
- passwords, tokens or stream URLs that reach the chat, the log or other users,
- `install.sh` or `tsmusic setup` installing something that does not match its checksum, or leaving files open to other users,
- the systemd unit giving the bot more access than it needs.

Problems in the programs the bot runs or connects to (TeamSpeak, yt-dlp, ffmpeg, deno) belong to their projects. Tell us as well when the bot makes such a problem worse or easier to reach.
