//! `youtube` source: `yt-dlp` as a process (separate arguments, `--` before the phrase).

use std::{
    io::{self, Read},
    num::NonZeroUsize,
    process::{Child, ChildStdout, ExitStatus, Stdio},
    str::FromStr,
    thread::{self, JoinHandle},
    time::Duration,
};

use rustix::process::Signal;
use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, warn};
use url::Url;

use crate::{
    audio::{AudioInput, read_tail},
    config::YoutubeConfig,
    process::{self, Stopper, TermOnDrop},
    sources::{Found, Location, Playlist, QueryType, TrackRef, cookies::CookieCopy},
};

const YT_VIDEO_URL: &str = "https://www.youtube.com/watch?v=";
const YT_PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(20);
/// The first with the preferred clients, the rest with the `yt-dlp` defaults.
const OPEN_ATTEMPTS: u32 = 3;
/// Seconds `yt-dlp` waits for a silent connection while streaming before it gives up,
/// so a stalled stream ends (and is retried or fails) instead of playing silence forever.
const STREAM_SOCKET_TIMEOUT: &str = "20";

/// Never contains the rejected text, the message may go to the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid YouTube id")]
pub struct InvalidId;

/// Always 11 characters of `[A-Za-z0-9_-]`, checked when created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoId(String);

impl FromStr for VideoId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.len() == 11
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidId)
        }
    }
}

impl VideoId {
    pub fn url(&self) -> String {
        format!("{YT_VIDEO_URL}{}", self.0)
    }
}

/// 2 to 64 characters of [A-Za-z0-9_-], checked when created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistId(String);

impl FromStr for PlaylistId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = (2..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidId)
        }
    }
}

impl PlaylistId {
    pub fn url(&self) -> String {
        format!("{YT_PLAYLIST_URL}{}", self.0)
    }
}

/// What a YouTube link points to. Only used while resolving `!song add`.
#[derive(Debug, PartialEq, Eq)]
pub enum YoutubeLink {
    Video(VideoId),
    Playlist(PlaylistId),
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("not a YouTube link")]
    NotYoutube,
    #[error("this YouTube link does not point to a video")]
    NoVideo,
    #[error("this YouTube link does not point to a playlist")]
    NoPlaylist,
    #[error("YouTube mixes are not supported, add a single video or a playlist")]
    MixNotSupported,
    #[error(transparent)]
    InvalidId(#[from] InvalidId),
}

/// Hosts that `sources/mod.rs` sends here. Compared exactly, a lookalike host never passes.
pub const HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

/// A link with both a video and a playlist (`watch?v=..&list=..`) gives the video,
/// a whole playlist is only added on purpose.
pub fn parse_link(url: &Url) -> Result<YoutubeLink, LinkError> {
    let host = url.host_str().unwrap_or_default();
    if !HOSTS.contains(&host) {
        return Err(LinkError::NotYoutube);
    }
    let mut segments = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty());

    let id: String = match (host, segments.next(), segments.next()) {
        ("youtu.be", Some(id), _) => id.to_owned(),
        (_, Some("watch"), _) => url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
            .ok_or(LinkError::NoVideo)?,
        (_, Some("shorts" | "embed" | "live"), Some(id)) => id.to_owned(),
        (_, Some("playlist"), _) => {
            let id = url
                .query_pairs()
                .find(|(key, _)| key == "list")
                .map(|(_, value)| value.into_owned())
                .ok_or(LinkError::NoPlaylist)?;
            if id.starts_with("RD") {
                return Err(LinkError::MixNotSupported);
            }
            return Ok(YoutubeLink::Playlist(id.parse()?));
        }
        _ => return Err(LinkError::NoVideo),
    };
    Ok(YoutubeLink::Video(id.parse()?))
}

/// Messages go to the chat, so they never contain `yt-dlp` output or paths.
/// Details stay in `source()` and `stderr` for the log.
#[derive(Debug, thiserror::Error)]
pub enum YoutubeError {
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    InvalidId(#[from] InvalidId),
    #[error("yt-dlp could not be started")]
    Spawn(#[source] std::io::Error),
    #[error("YouTube did not answer in time")]
    Timeout,
    #[error("YouTube lookup failed")]
    Failed { status: ExitStatus, stderr: String },
    #[error("YouTube lookup failed")]
    Json(#[source] serde_json::Error),
    #[error("nothing found on YouTube")]
    NoResults,
    /// Needs a signed-in account with a confirmed age, no client helps without one.
    #[error("this video is age-restricted")]
    AgeRestricted,
}

/// Finds the track or the playlist, first with `player_clients`, then with the `yt-dlp`
/// defaults if that fails. A playlist gives at most `max_playlist_len` tracks.
pub async fn resolve(
    config: &YoutubeConfig,
    query: &QueryType,
    max_playlist_len: NonZeroUsize,
) -> Result<Found, YoutubeError> {
    match resolve_with(config, query, max_playlist_len, Clients::Preferred).await {
        Err(YoutubeError::Failed { .. }) if !config.player_clients.is_empty() => {
            warn!("yt-dlp lookup failed with the preferred clients, retrying with its defaults");
            resolve_with(config, query, max_playlist_len, Clients::Default).await
        }
        result => result,
    }
}

/// One lookup with the given clients, no retry. For `yt-check`, which tests each path on its own.
pub async fn resolve_with(
    config: &YoutubeConfig,
    query: &QueryType,
    max_playlist_len: NonZeroUsize,
    clients: Clients,
) -> Result<Found, YoutubeError> {
    let (target, limit) = match query {
        QueryType::Link(url) => match parse_link(url)? {
            YoutubeLink::Video(video_id) => (video_id.url(), None),
            YoutubeLink::Playlist(playlist_id) => (playlist_id.url(), Some(max_playlist_len)),
        },
        QueryType::Phrase(phrase) => (format!("ytsearch1:{phrase}"), None),
    };
    let json = lookup(config, &target, limit, clients).await?;
    let mut found = parse_output(&json)?;
    match query {
        QueryType::Phrase(_) => {
            found.tracks.truncate(1);
            found.playlist = None;
        }
        // A playlist link always keeps its playlist, even with one video or a short limit.
        QueryType::Link(_) if limit.is_none() => found.playlist = None,
        QueryType::Link(_) => {}
    }
    Ok(found)
}

/// One `yt-dlp` run that prints JSON about `target`, a playlist cut to `limit` entries.
async fn lookup(
    config: &YoutubeConfig,
    target: &str,
    limit: Option<NonZeroUsize>,
    clients: Clients,
) -> Result<Vec<u8>, YoutubeError> {
    let YtDlp {
        command: mut cmd,
        cookies: _cookies,
    } = yt_dlp_command(config, clients);
    // Private and deleted videos are left out of playlists instead of listed under a placeholder.
    cmd.args(["--flat-playlist", "--dump-single-json"])
        .args(["--compat-options", "no-youtube-unavailable-videos"]);
    if let Some(limit) = limit {
        cmd.arg("--playlist-items").arg(format!("1:{limit}"));
    }
    cmd.arg("--")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Stopped when the lookup is dropped: on timeout or when its task is aborted.
    let mut child = TermOnDrop::new(Command::from(cmd).spawn().map_err(YoutubeError::Spawn)?);

    let output = tokio::time::timeout(LOOKUP_TIMEOUT, child.output())
        .await
        .map_err(|_| YoutubeError::Timeout)?
        .map_err(YoutubeError::Spawn)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        // A recognised reason is in the error itself, the full output only helps with debugging.
        if is_age_restricted(&stderr) {
            debug!("yt-dlp lookup failed with {}: {stderr}", output.status);
            return Err(YoutubeError::AgeRestricted);
        }
        warn!("yt-dlp lookup failed with {}: {stderr}", output.status);
        return Err(YoutubeError::Failed {
            status: output.status,
            stderr,
        });
    }
    Ok(output.stdout)
}

/// The reasons `yt-dlp` gives for an age gate, the same ones it looks for itself (`_is_agegated`).
fn is_age_restricted(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    ["confirm your age", "age-restricted", "age-verification"]
        .iter()
        .any(|reason| stderr.contains(reason))
}

/// Which YouTube clients a `yt-dlp` run may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clients {
    /// `player_clients` from the config, or the defaults when that is empty.
    Preferred,
    /// Whatever `yt-dlp` chooses, it follows YouTube changes with each release.
    Default,
}

/// A `yt-dlp` run before it starts.
struct YtDlp {
    /// `std` command, `lookup` turns it into a tokio one.
    command: std::process::Command,
    /// Keep it until the process has exited, dropping it removes the file.
    cookies: Option<CookieCopy>,
}

/// Options shared by every `yt-dlp` run. Callers add their own options, then `--` and the target.
fn yt_dlp_command(config: &YoutubeConfig, clients: Clients) -> YtDlp {
    let mut command = std::process::Command::new(&config.yt_dlp_path);
    command.args(["--ignore-config", "--no-warnings"]);
    let preferred = clients == Clients::Preferred && !config.player_clients.is_empty();
    if preferred {
        command.arg("--extractor-args").arg(format!(
            "youtube:player_client={}",
            config.player_clients.join(",")
        ));
    }
    // The preferred clients (`visionos`) do not take cookies, so the account is used only
    // when the `yt-dlp` defaults are.
    let cookies = config
        .cookies_file
        .as_deref()
        .filter(|_| !preferred)
        .and_then(|path| match CookieCopy::new(path) {
            Ok(cookies) => Some(cookies),
            Err(err) => {
                warn!(
                    "{:#}, yt-dlp runs without cookies",
                    anyhow::Error::from(err)
                );
                None
            }
        });
    if let Some(cookies) = &cookies {
        command.arg("--cookies").arg(cookies.path());
    }
    YtDlp { command, cookies }
}

/// The two shapes of `yt-dlp --flat-playlist --dump-single-json`, told apart by `_type`.
/// Any other `_type` is a `Json` error, an unknown shape is never guessed.
#[derive(Deserialize)]
#[serde(tag = "_type", rename_all = "lowercase")]
enum Output {
    /// A link to a single video.
    Video(Entry),
    /// A playlist, or a `ytsearch1:` search that is empty when nothing was found.
    Playlist {
        #[serde(default)]
        title: Option<String>,
        /// The whole playlist, `entries` holds at most the requested part of it.
        #[serde(default)]
        playlist_count: Option<usize>,
        entries: Vec<Entry>,
    },
}

/// Only the fields the bot needs, `yt-dlp` sends hundreds more.
#[derive(Deserialize)]
struct Entry {
    id: String,
    title: String,
}

impl Entry {
    fn into_track(self) -> Result<TrackRef, InvalidId> {
        Ok(TrackRef {
            location: Location::Youtube(self.id.parse()?),
            title: self.title,
        })
    }
}

/// The id is checked again, the output of `yt-dlp` is not trusted.
/// A playlist shape always gives `playlist`, `resolve_with` drops it for a search.
// TODO: reject upcoming streams (`live_status` "is_upcoming"), they cannot be played yet.
pub(super) fn parse_output(json: &[u8]) -> Result<Found, YoutubeError> {
    let found = match serde_json::from_slice(json).map_err(YoutubeError::Json)? {
        Output::Video(entry) => Found::track(entry.into_track()?),
        Output::Playlist {
            title,
            playlist_count,
            entries,
        } => Found {
            tracks: entries
                .into_iter()
                .map(Entry::into_track)
                .collect::<Result<_, _>>()?,
            playlist: Some(Playlist {
                title: title.unwrap_or_default(),
                total: playlist_count,
            }),
        },
    };
    if found.tracks.is_empty() {
        return Err(YoutubeError::NoResults);
    }
    Ok(found)
}

/// Starts `yt-dlp` writing the audio of `id` to its stdout, nothing is saved to disk.
///
/// `std::process`, not tokio: the audio side reads it with a plain `Read` on its own thread.
/// Starts with `player_clients`, retries with the `yt-dlp` defaults while no audio has come.
/// `stop` ends the processes at once and prevents retries.
pub fn open(
    config: &YoutubeConfig,
    id: &VideoId,
    stop: &Stopper,
) -> Result<AudioInput, YoutubeError> {
    start(config, id, Clients::Preferred, OPEN_ATTEMPTS, stop.clone())
}

/// One attempt with the given clients, no retry. For `yt-check`, which tests each path on its own.
pub fn open_with(
    config: &YoutubeConfig,
    id: &VideoId,
    clients: Clients,
    stop: &Stopper,
) -> Result<AudioInput, YoutubeError> {
    start(config, id, clients, 1, stop.clone())
}

/// Spawns the first `yt-dlp`, the reader spawns the rest, up to `attempts` in total.
fn start(
    config: &YoutubeConfig,
    id: &VideoId,
    clients: Clients,
    attempts: u32,
    stop: Stopper,
) -> Result<AudioInput, YoutubeError> {
    let stream = Stream {
        config: config.clone(),
        url: id.url(),
    };
    let running = stream.spawn(clients, &stop).map_err(YoutubeError::Spawn)?;
    Ok(AudioInput::Stream(Box::new(ChildReader {
        stream,
        attempt: 1,
        attempts,
        started: false,
        running,
        stop,
    })))
}

/// What is needed to start `yt-dlp` again for the same video.
struct Stream {
    config: YoutubeConfig,
    url: String,
}

impl Stream {
    /// WebM is preferred, it plays from a pipe, M4A may need to seek. Some videos have no
    /// audio-only format for a client (seen with `visionos` on videos for kids), then a
    /// format with video up to 360p, the decoder skips the video. Its AAC is close to
    /// the audio-only formats, a higher resolution would only cost bandwidth.
    /// The process is watched by `stop`: SIGTERM, so `yt-dlp` still cleans up after itself.
    fn spawn(&self, clients: Clients, stop: &Stopper) -> io::Result<Running> {
        let YtDlp {
            mut command,
            cookies,
        } = yt_dlp_command(&self.config, clients);
        let mut child = command
            .args([
                "--no-playlist",
                "--quiet",
                "--no-progress",
                "--socket-timeout",
                STREAM_SOCKET_TIMEOUT,
                "--format",
                "bestaudio[ext=webm]/bestaudio/best[height<=360]",
                "--output",
                "-",
            ])
            .arg("--")
            .arg(&self.url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        stop.watch(&child, Signal::TERM);

        // Both are `Some`, they were set to `piped` above.
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        // Drained on a thread, a full stderr pipe would stop `yt-dlp`.
        let stderr = thread::spawn(move || read_tail(&mut stderr));

        Ok(Running {
            child,
            stdout,
            stderr: Some(stderr),
            _cookies: cookies,
        })
    }
}

/// The audio of a video, starting `yt-dlp` again while nothing has been read yet.
///
/// After the first byte ffmpeg already has the start of the file, a new process
/// would send it from the beginning again, so a later failure stays an error.
struct ChildReader {
    stream: Stream,
    /// Counts from 1, retries always use the `yt-dlp` defaults.
    attempt: u32,
    /// Attempts allowed in total, 1 means no retry.
    attempts: u32,
    started: bool,
    running: Running,
    /// A stopped playback signalled the process, its end is no failure and is not retried.
    stop: Stopper,
}

impl Read for ChildReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let n = self.running.stdout.read(buf)?;
            if n > 0 || buf.is_empty() {
                self.started |= n > 0;
                return Ok(n);
            }
            if self.stop.is_stopped() {
                return Ok(0);
            }
            match self.running.finish() {
                Ok(()) => return Ok(0),
                Err(failure)
                    if failure.can_retry() && !self.started && self.attempt < self.attempts =>
                {
                    self.attempt += 1;
                    warn!(
                        "yt-dlp gave no audio, retrying with its default clients ({}/{})",
                        self.attempt, self.attempts
                    );
                    // The old process has exited, replacing it reaps it in `Drop`.
                    self.running = self.stream.spawn(Clients::Default, &self.stop)?;
                }
                Err(failure) => return Err(failure.into()),
            }
        }
    }
}

/// One running `yt-dlp`. Dropping it stops the process, see `process::stop`.
struct Running {
    child: Child,
    stdout: ChildStdout,
    /// Taken when the process ends, so the tail is logged once.
    stderr: Option<JoinHandle<String>>,
    /// Fields drop after `drop` has stopped the process, so it never loses its cookies.
    _cookies: Option<CookieCopy>,
}

impl Running {
    /// A stream that ends because `yt-dlp` failed is an error, not the end of the track.
    fn finish(&mut self) -> Result<(), Failure> {
        let status = self.child.wait().map_err(Failure::Wait)?;
        if status.success() {
            return Ok(());
        }
        // Taken once, a second call after the failure only sees the exit status.
        let tail = self
            .stderr
            .take()
            .map(|stderr| stderr.join().unwrap_or_default())
            .unwrap_or_default();
        // The player logs the failure with the title, the output is only needed when
        // the reason is not recognised.
        if is_age_restricted(&tail) {
            debug!("yt-dlp failed with {status}: {tail}");
            Err(Failure::AgeRestricted)
        } else {
            warn!("yt-dlp failed with {status}: {tail}");
            Err(Failure::Exit(status))
        }
    }
}

/// Why a `yt-dlp` stream ended with an error.
enum Failure {
    Wait(io::Error),
    /// Another client does not help, so it is not retried.
    AgeRestricted,
    Exit(ExitStatus),
}

impl Failure {
    fn can_retry(&self) -> bool {
        matches!(self, Failure::Exit(_))
    }
}

/// The decoder only takes `io::Error`, the message is kept for the log.
impl From<Failure> for io::Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Wait(err) => err,
            Failure::AgeRestricted => io::Error::other(YoutubeError::AgeRestricted),
            Failure::Exit(status) => io::Error::other(format!("yt-dlp exited with {status}")),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // Stops `yt-dlp` if the track was skipped and reaps it to avoid a zombie process.
        process::stop(&mut self.child);
    }
}
