//! `youtube` source: `yt-dlp` as a process (separate arguments, `--` before the phrase).

use std::{
    io::{self, Read},
    process::{Child, ChildStdout, ExitStatus, Stdio},
    str::FromStr,
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Deserialize;
use tokio::process::Command;
use tracing::warn;
use url::Url;

use crate::{
    audio::{AudioInput, read_tail},
    config::YoutubeConfig,
    sources::{Location, QueryType, TrackRef},
};

const YT_URL: &str = "https://www.youtube.com/watch?v=";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(20);
/// The first with the preferred clients, the rest with the `yt-dlp` defaults.
const OPEN_ATTEMPTS: u32 = 3;

/// Never contains the rejected text, the message may go to the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid YouTube video id")]
pub struct InvalidVideoId;

/// Always 11 characters of `[A-Za-z0-9_-]`, checked when created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoId(String);

impl FromStr for VideoId {
    type Err = InvalidVideoId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.len() == 11
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidVideoId)
        }
    }
}

impl VideoId {
    pub fn url(&self) -> String {
        format!("{YT_URL}{}", self.0)
    }
}

/// What a YouTube link points to. Only used while resolving `!song add`.
#[derive(Debug, PartialEq, Eq)]
pub enum YoutubeLink {
    Video(VideoId),
    // TODO: Playlist(PlaylistId) in stage 5.
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("not a YouTube link")]
    NotYoutube,
    #[error("this YouTube link does not point to a video")]
    NoVideo,
    #[error("YouTube playlists are not supported yet")]
    PlaylistNotSupported,
    #[error(transparent)]
    InvalidId(#[from] InvalidVideoId),
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
        (_, Some("playlist"), _) => return Err(LinkError::PlaylistNotSupported),
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
    InvalidId(#[from] InvalidVideoId),
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

/// Finds the track, first with `player_clients`, then with the `yt-dlp` defaults if that fails.
pub async fn resolve(config: &YoutubeConfig, query: &QueryType) -> Result<TrackRef, YoutubeError> {
    match resolve_with(config, query, Clients::Preferred).await {
        Err(YoutubeError::Failed { .. }) if !config.player_clients.is_empty() => {
            warn!("yt-dlp lookup failed with the preferred clients, retrying with its defaults");
            resolve_with(config, query, Clients::Default).await
        }
        result => result,
    }
}

/// One lookup with the given clients, no retry. For `yt-check`, which tests each path on its own.
pub async fn resolve_with(
    config: &YoutubeConfig,
    query: &QueryType,
    clients: Clients,
) -> Result<TrackRef, YoutubeError> {
    let target = match query {
        QueryType::Link(url) => match parse_link(url)? {
            YoutubeLink::Video(video_id) => video_id.url(),
        },
        QueryType::Phrase(phrase) => format!("ytsearch1:{phrase}"),
    };
    let json = lookup(config, &target, clients).await?;
    parse_output(&json)
}

/// One `yt-dlp` run that prints JSON about `target`.
async fn lookup(
    config: &YoutubeConfig,
    target: &str,
    clients: Clients,
) -> Result<Vec<u8>, YoutubeError> {
    let mut cmd = yt_dlp_command(config, clients);
    cmd.args(["--flat-playlist", "--dump-single-json"])
        .arg("--")
        .arg(target)
        .stdin(Stdio::null());
    let mut cmd = Command::from(cmd);
    // `timeout` drops the future when time runs out, this kills the process with it.
    cmd.kill_on_drop(true);

    let output = tokio::time::timeout(LOOKUP_TIMEOUT, cmd.output())
        .await
        .map_err(|_| YoutubeError::Timeout)?
        .map_err(YoutubeError::Spawn)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        warn!("yt-dlp lookup failed with {}: {stderr}", output.status);
        if is_age_restricted(&stderr) {
            return Err(YoutubeError::AgeRestricted);
        }
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

/// Options shared by every `yt-dlp` run. Callers add their own options, then `--` and the target.
/// `std` command, `lookup` turns it into a tokio one.
fn yt_dlp_command(config: &YoutubeConfig, clients: Clients) -> std::process::Command {
    let mut cmd = std::process::Command::new(&config.yt_dlp_path);
    cmd.args(["--ignore-config", "--no-warnings"]);
    if clients == Clients::Preferred && !config.player_clients.is_empty() {
        cmd.arg("--extractor-args").arg(format!(
            "youtube:player_client={}",
            config.player_clients.join(",")
        ));
    }
    cmd
}

/// The two shapes of `yt-dlp --flat-playlist --dump-single-json`, told apart by `_type`.
/// Any other `_type` is a `Json` error, an unknown shape is never guessed.
#[derive(Deserialize)]
#[serde(tag = "_type", rename_all = "lowercase")]
enum Output {
    /// A link to a single video.
    Video(Entry),
    /// A `ytsearch1:` search, empty when nothing was found.
    Playlist { entries: Vec<Entry> },
}

/// Only the fields the bot needs, `yt-dlp` sends hundreds more.
#[derive(Deserialize)]
struct Entry {
    id: String,
    title: String,
}

/// The id is checked again, the output of `yt-dlp` is not trusted.
// TODO: reject live streams (`live_status`), they never end and would block the queue.
pub(super) fn parse_output(json: &[u8]) -> Result<TrackRef, YoutubeError> {
    let entry = match serde_json::from_slice(json).map_err(YoutubeError::Json)? {
        Output::Video(entry) => entry,
        Output::Playlist { entries } => {
            entries.into_iter().next().ok_or(YoutubeError::NoResults)?
        }
    };
    Ok(TrackRef {
        location: Location::Youtube(entry.id.parse()?),
        title: entry.title,
    })
}

/// Starts `yt-dlp` writing the audio of `id` to its stdout, nothing is saved to disk.
///
/// `std::process`, not tokio: the audio side reads it with a plain `Read` on its own thread.
/// Starts with `player_clients`, retries with the `yt-dlp` defaults while no audio has come.
pub fn open(config: &YoutubeConfig, id: &VideoId) -> Result<AudioInput, YoutubeError> {
    start(config, id, Clients::Preferred, OPEN_ATTEMPTS)
}

/// One attempt with the given clients, no retry. For `yt-check`, which tests each path on its own.
pub fn open_with(
    config: &YoutubeConfig,
    id: &VideoId,
    clients: Clients,
) -> Result<AudioInput, YoutubeError> {
    start(config, id, clients, 1)
}

/// Spawns the first `yt-dlp`, the reader spawns the rest, up to `attempts` in total.
fn start(
    config: &YoutubeConfig,
    id: &VideoId,
    clients: Clients,
    attempts: u32,
) -> Result<AudioInput, YoutubeError> {
    let stream = Stream {
        config: config.clone(),
        url: id.url(),
    };
    let running = stream.spawn(clients).map_err(YoutubeError::Spawn)?;
    Ok(AudioInput::Stream(Box::new(ChildReader {
        stream,
        attempt: 1,
        attempts,
        started: false,
        running,
    })))
}

/// What is needed to start `yt-dlp` again for the same video.
struct Stream {
    config: YoutubeConfig,
    url: String,
}

impl Stream {
    /// WebM is preferred, it plays from a pipe, M4A may need to seek.
    fn spawn(&self, clients: Clients) -> io::Result<Running> {
        let mut child = yt_dlp_command(&self.config, clients)
            .args([
                "--no-playlist",
                "--quiet",
                "--no-progress",
                "--format",
                "bestaudio[ext=webm]/bestaudio",
                "--output",
                "-",
            ])
            .arg("--")
            .arg(&self.url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Both are `Some`, they were set to `piped` above.
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        // Drained on a thread, a full stderr pipe would stop `yt-dlp`.
        let stderr = thread::spawn(move || read_tail(&mut stderr));

        Ok(Running {
            child,
            stdout,
            stderr: Some(stderr),
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
}

impl Read for ChildReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let n = self.running.stdout.read(buf)?;
            if n > 0 || buf.is_empty() {
                self.started |= n > 0;
                return Ok(n);
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
                    self.running = self.stream.spawn(Clients::Default)?;
                }
                Err(failure) => return Err(failure.into()),
            }
        }
    }
}

/// One running `yt-dlp`. Dropping it kills the process.
struct Running {
    child: Child,
    stdout: ChildStdout,
    /// Taken when the process ends, so the tail is logged once.
    stderr: Option<JoinHandle<String>>,
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
        warn!("yt-dlp failed with {status}: {tail}");
        if is_age_restricted(&tail) {
            Err(Failure::AgeRestricted)
        } else {
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
        // Kills `yt-dlp` if the track was skipped and reaps it to avoid a zombie process.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
