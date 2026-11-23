# Controlling the music bot

> **Status:** the bot is under development. This guide describes the commands as designed; it will be updated as they are implemented.

The bot joins a TeamSpeak channel as a regular client and plays music into it. You control it by typing commands in the **channel chat** or by sending it a **private message** (double-click the bot to open a private chat).

Commands are grouped into three families:

- `!song`: playback
- `!queue`: the queue
- `!bot`: the bot itself

## Getting help

- `!help` lists the three families.
- `!song help`, `!queue help` and `!bot help` list the commands of one family, with a short description of each. A bare `!song`, `!queue` or `!bot`, or `--help` instead of `help`, gives the same list.
- `!song add --help` explains the options of `!song add`, and `!bot config help` the settings commands.
- A command with a typo in its first word, such as `!sond add`, gets the answer `unknown command, did you mean !song?`. Commands that are not close to any of the bot's commands are ignored, so the bot does not answer commands meant for another bot in the channel.

## Getting the bot into your channel

- When idle, the bot waits in its **home channel**.
- **Anyone** can take the bot from its home channel to their own channel:
  - **drag it** to your channel (recommended; requires the matching server permissions), or
  - send it `!bot summon` in a **private message** and it will join your channel.
- While the bot is in someone else's channel, it is **busy**: people outside that channel cannot move it or control it. Only admins can.
- When the bot joins a channel with people in it, it plays a short **welcome** sound.
- The bot leaves on its own and returns to its home channel when:
  - nothing has been playing for **10 minutes** (a paused song counts as nothing playing), or
  - it has been **alone** in the channel for **1 minute**.

  Before leaving, it plays a **goodbye** sound. Starting a song while the goodbye is playing cancels the return. When the bot returns home, **the queue is cleared**.

## Who can use commands

- **Users** in the **same channel** as the bot.
- **Admins** from anywhere on the server. Admins can also move the bot out of a channel where it is busy, and they are the only ones who can use `!bot config`.

Who counts as an admin or a user is set in the bot configuration, by TeamSpeak **UID** and/or **server group** (by name or ID). By default everyone is a user. Some roles or people can be **excluded**: they cannot control or summon the bot. Ask your server admin if you are unsure of your role.

Commands from other channels are rejected with a short message.

## Command overview

| Command | What it does |
| --- | --- |
| `!song add <link>` | Adds whatever the link points to (a track, an album or a playlist) |
| `!song add <search phrase>` | Searches for a track and adds the first result |
| `!song add --album <search phrase>` | Searches for an album and adds all its tracks |
| `!song add --playlist <search phrase>` | Searches public playlists and adds all tracks of the first result |
| `!song add --source <source> ...` | Uses the given source instead of the default one |
| `!song stop` / `!song pause` | Pauses the current song; the queue is kept |
| `!song resume` | Resumes the paused song, or starts the next one in the queue |
| `!song next` / `!song skip` | Skips to the next song in the queue |
| `!song current` | Shows the song that is playing now |
| `!queue show` / `!queue display` | Shows the current song and the numbered queue |
| `!queue remove <number>` | Removes the song with that number from the queue |
| `!queue remove last` | Removes the last song in the queue |
| `!queue clear` | Stops the current song and empties the whole queue |
| `!bot summon` | *(private message)* Brings the bot from its home channel to yours |
| `!bot volume [0-100]` | Shows the volume, or sets it for all songs |
| `!bot ping` | Checks that the bot answers |
| `!bot config list [section]` | *(admins only, private message)* Lists the bot settings |
| `!bot config get <key>` | *(admins only, private message)* Shows one setting |
| `!bot config set <key> <value>` | *(admins only, private message)* Changes a bot setting |
| `!bot config reload` | *(admins only, private message)* Reloads settings after editing `config.toml` by hand |
| `!help` | Lists the command families, see [Getting help](#getting-help) |

## `!song` - playback

### `!song add`

```text
!song add [--track | --album | --playlist] [--source <source>] <link | search phrase>
```

Examples:

```text
!song add https://tidal.com/browse/track/12345678
!song add https://tidal.com/browse/album/12345678
!song add https://tidal.com/browse/playlist/0a1b2c3d-...
!song add get lucky
!song add daft punk get lucky
!song add --album random access memories
!song add --playlist chill evening
!song add --source youtube get lucky daft punk
```

**Links** work for everything. The bot recognizes the service and whether the link points to a track, an album or a playlist. You don't need any flags for links. An album or playlist link adds all its tracks in the original order.

YouTube links:

- `youtube.com/watch?v=...`, `youtu.be/...`, `/shorts/...`, `/live/...` and `music.youtube.com/watch?v=...` add one video.
- `youtube.com/playlist?list=...` (also on `music.youtube.com`) adds the whole playlist. Private and deleted videos are skipped.
- A video opened from a playlist (`watch?v=...&list=...`) adds **only that video**, so you don't get a whole playlist by accident. To add the playlist, open it and copy the link of the playlist page.
- YouTube **mixes** (playlists starting with `RD`, made by YouTube while you listen) are not supported. Add a single video or a regular playlist instead.
- One playlist link adds at most **100** songs by default (the bot administrator may change the limit). The rest of the playlist is left out.
- Live streams play until you skip them with `!song next`. Streams that have not started yet cannot be played.

**Search phrases** are free text, passed to the service's search just as you typed them. You can write the title alone, the artist and the title, the title and the artist, and so on. The bot adds the **first result**.

- By default the bot searches for a **track** (`--track`).
- Use `--album` or `--playlist` to search for an album or a playlist instead. The whole album or playlist is added. `--playlist` searches **public playlists** only.
- The search uses the **default source** (YouTube, unless changed in the configuration). Use `--source` to pick another one:

| `--source` value | What it plays | Status |
| --- | --- | --- |
| `tidal` | Tidal | Planned |
| `spotify` | Spotify | Planned |
| `youtube` | YouTube / YouTube Music (default) | First to be supported |
| `soundcloud` | SoundCloud | Planned |
| `local` | Audio files stored on the bot's server | Supported |

After adding:

- If nothing is playing, the song starts immediately. Otherwise it goes to the **end of the queue**. Adding a single song gives no reply, the bot only answers when something goes wrong.
- After a playlist the bot says how many songs it added, e.g. `added 37 songs from Chill Evening`, or `added 100 of 182 songs from ...` when part of the playlist was over the limit.
- Searching and adding happen in the background, so you can keep using commands in the meantime. Songs keep the order of your commands, even when a later search finishes first.
- If the queue fills up in the middle of a playlist, the bot adds what fits and tells you how many songs that was.
- If a track is unavailable, the bot says so and moves on to the next one.
- The queue has no length limit by default (the bot administrator may set one).

### `!song stop` / `!song pause`

Both commands do the same thing: they pause the current song and keep the queue.

### `!song resume`

Continues the paused song from where it stopped. If no song is paused, it starts the next song in the queue. If the queue is empty too, the bot replies that the queue is empty.

### `!song next` / `!song skip`

Both commands do the same thing: they skip the current song, including one that is still loading, and play the next one. If the queue is empty, playback stops and the bot tells you so.

### `!song current`

Replies with the title of the song that is playing now, with `(paused)` after it when it is paused, or `nothing is playing`.

## `!queue` - the queue

### `!queue show` / `!queue display`

Shows the song that is playing now and the upcoming songs, numbered from 1. A long queue is split into pages: `!queue show 2` shows the second page.

### `!queue remove <number>` / `!queue remove last`

Removes the song with the given number, as shown by `!queue show`. `!queue remove last` removes the song at the end of the queue, for example one you have just added by mistake, without looking up its number. To skip the song that is playing now, use `!song next`.

### `!queue clear`

Clears everything: it stops the current song (including one that is still loading), removes all songs from the queue and stops an album or playlist that is still being added.

To only skip the current song use `!song next`; to only stop it use `!song pause`.

## `!bot` - the bot

### `!bot summon`

Send `!bot summon` to the bot in a private message. If the bot is in its home channel, it joins your channel. If it is busy in another channel, it refuses, unless you are an admin.

### `!bot volume [0-100]`

Without a number, the bot replies with the current volume. With a number, it sets the volume for all songs, for everyone in the channel; a `%` at the end is fine (`!bot volume 40%`). The change is heard right away, also in the middle of a song.

The scale is logarithmic, like most volume controls: 50 is clearly quieter than 100, and below about 20 it gets very quiet. The bot administrator may set a highest allowed volume. The volume goes back to the default when the bot restarts.

### `!bot ping`

The bot replies `pong`. Use it to check that the bot is connected and reads the chat.

### `!bot config` (admins only)

The bot accepts these commands only from admins, and only in a private message, so that settings are not shown to the whole channel.

```text
!bot config list
!bot config list events
!bot config get events.on_idle.delay
!bot config set events.on_idle.delay 15m
!bot config set sources.default youtube
!bot config reload
```

- `list` lists the settings, all of them or one section. These are the same settings as in the bot's `config.toml` file.
- `get` shows the value of one setting, given as `section.key`.
- Secrets (passwords, tokens) are not part of these settings. They are kept in a separate, protected file and cannot be viewed or changed from the chat.
- `set` checks the new value, applies it and saves it to `config.toml`. Some settings, such as the server address, only take effect after the bot restarts; the bot tells you when that is the case.
- `reload` reads `config.toml` again after you have edited it by hand. If the file has an error, the bot keeps its current settings and tells you where the error is.

## Planned commands

These commands are not designed in detail yet:

- `!song search <phrase>` and `!song pick <n>`: choose a search result from a list
