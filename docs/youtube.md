# YouTube cookies

For the administrator of the server the bot runs on.

The bot plays YouTube without an account. Two cases need the cookies of a signed-in YouTube account:

- **YouTube blocks the server.** YouTube often blocks the addresses of data centers, so a bot on a VPS may fail where the same bot works at home. The log then shows `yt-dlp lookup failed` with `Sign in to confirm you're not a bot`, and `tsmusic yt-check` fails on every path.
- **Age-restricted videos.** They need an account with a confirmed age. Without one the bot answers `this video is age-restricted`.

> **Use a separate Google account.** YouTube may block an account used by a bot, for a while or for good. The cookie file also gives full access to the account, so keep it as private as `secrets.toml`.

## Exporting the cookies

Follow [Exporting YouTube cookies](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies) in the yt-dlp wiki. In short: sign in to YouTube in a private browser window, export the `youtube.com` cookies with an extension listed in [How do I pass cookies to yt-dlp?](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp), then close the window and never open that session again. YouTube changes the cookies of a session that stays open in a browser, and the exported ones then stop working.

The file must be in the Netscape format, the first line is `# Netscape HTTP Cookie File` or `# HTTP Cookie File`.

## Setting up the bot

1. Copy the file next to the config and make it private to the bot:

   ```sh
   sudo install -o tsmusic -g tsmusic -m 600 cookies.txt /etc/tsmusic/youtube-cookies.txt
   ```

   Then delete the exported file from your computer and from any place you copied it through.
2. In `/etc/tsmusic/config.toml`:

   ```toml
   [sources.youtube]
   player_clients = []
   cookies_file = "youtube-cookies.txt"
   ```

   A relative path is relative to the config file. `player_clients = []` matters on a server that YouTube blocks: the preferred clients do not take cookies, so without this line every song would first fail without the account.
3. Check it as the bot's user. The path with cookies must work:

   ```sh
   sudo -u tsmusic tsmusic --config /etc/tsmusic/config.toml yt-check "daft punk get lucky"
   ```

4. Restart the bot: `sudo systemctl restart tsmusic`

The bot does not start when the file is missing, is readable by other users or belongs to another user. The error names the file and the fix.

## How the bot uses the file

- The cookies go only to the runs of yt-dlp that use its default clients. With `player_clients` set, that is the retry after the preferred clients failed. With `player_clients = []`, every run.
- yt-dlp writes its cookie file back when it exits, which two runs at once could break. So the bot never passes your file: each run gets its own copy in a private temporary directory, removed when the run ends. Your file is only read.
- The file is read again for every run. To replace expired cookies, install a new export over the old file. No restart is needed.

## When the cookies stop working

The log shows `Sign in to confirm you're not a bot` again, or yt-dlp says the account cookies are no longer valid. Export new cookies as described above, from a new private window, and replace the file.
