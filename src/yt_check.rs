//! `tsmusic yt-check`: the whole way from a phrase or link to decoded audio, without TeamSpeak.
//!
//! Each set of YouTube clients is tested on its own, without the fallback the bot uses,
//! so a broken path shows up even when the other one still plays.

use std::{
    num::NonZeroUsize,
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    audio::{Decoder, FRAME_DURATION},
    config::{Config, YoutubeConfig},
    sources::{
        Location, QueryType, parse_query,
        youtube::{self, Clients, VideoId},
    },
};

/// Enough to see that audio flows, short enough to check both paths quickly.
const SAMPLE_LENGTH: Duration = Duration::from_secs(10);

pub async fn run(config: &Config, query: &str, full: bool) -> anyhow::Result<()> {
    let query = parse_query(query.trim());
    println!("\nquery");
    match &query {
        QueryType::Link(url) => println!("  link    {url}"),
        QueryType::Phrase(phrase) => println!("  phrase  {phrase:?}"),
    }

    let youtube = &config.sources.youtube;
    let mut working = Vec::new();
    for (label, clients) in paths(youtube) {
        println!("\nclients {label}");
        match check_path(config, &query, clients, full).await {
            Ok(()) => working.push(label),
            Err(err) => println!("  FAILED  {err:#}"),
        }
    }

    println!();
    match working.first() {
        None => anyhow::bail!("no path played this video, see the errors above"),
        Some(first) if working.len() > 1 => {
            println!("OK: both paths work, the bot uses {first} first")
        }
        Some(only) => println!("OK only with {only}, the bot falls back to it after a failure"),
    }
    Ok(())
}

/// The paths the bot can take: the configured clients first, then the yt-dlp defaults.
fn paths(youtube: &YoutubeConfig) -> Vec<(String, Clients)> {
    let defaults = ("yt-dlp defaults".to_owned(), Clients::Default);
    if youtube.player_clients.is_empty() {
        vec![defaults]
    } else {
        vec![
            (youtube.player_clients.join(","), Clients::Preferred),
            defaults,
        ]
    }
}

async fn check_path(
    config: &Config,
    query: &QueryType,
    clients: Clients,
    full: bool,
) -> anyhow::Result<()> {
    let youtube = config.sources.youtube.clone();
    let start = Instant::now();
    // Only the first video is played, so a playlist is not listed further.
    let found = youtube::resolve_with(&youtube, query, NonZeroUsize::MIN, clients).await?;
    if let Some(playlist) = &found.playlist {
        let total = playlist
            .total
            .map_or_else(|| "?".to_owned(), |total| total.to_string());
        println!(
            "  list    {:?}, {total} videos, checking the first",
            playlist.title
        );
    }
    let track = found
        .tracks
        .into_iter()
        .next()
        .expect("parse_output never returns an empty list");
    let Location::Youtube(id) = track.location else {
        anyhow::bail!("the lookup did not give a YouTube video");
    };
    println!(
        "  lookup  {} {:?}  {:.1}s",
        id.url(),
        track.title,
        start.elapsed().as_secs_f64()
    );

    let ffmpeg = config.paths.ffmpeg_path.clone();
    let limit = (!full).then_some(SAMPLE_LENGTH);
    // The decoder and the yt-dlp stream block, so they get their own thread.
    let decoded =
        tokio::task::spawn_blocking(move || decode(&ffmpeg, &youtube, &id, clients, limit))
            .await??;
    println!(
        "  stream  first audio after {:.1}s, decoded {:.1}s of audio in {:.1}s",
        decoded.first_audio.as_secs_f64(),
        decoded.audio.as_secs_f64(),
        decoded.took.as_secs_f64()
    );
    Ok(())
}

struct Decoded {
    first_audio: Duration,
    audio: Duration,
    took: Duration,
}

/// Streams the video through the same `Decoder` the bot uses, up to `limit` of audio.
fn decode(
    ffmpeg: &Path,
    youtube: &YoutubeConfig,
    id: &VideoId,
    clients: Clients,
    limit: Option<Duration>,
) -> anyhow::Result<Decoded> {
    let start = Instant::now();
    let input = youtube::open_with(youtube, id, clients)?;
    let mut decoder = Decoder::spawn(ffmpeg, input)?;
    let mut first_audio = None;
    let mut audio = Duration::ZERO;

    while limit.is_none_or(|limit| audio < limit) {
        if decoder.read_frame()?.is_none() {
            // Only at the real end: `finish` waits for ffmpeg, which runs on after a stop.
            // `AudioError` already puts its source in the message, `{:#}` would repeat it.
            decoder.finish().map_err(|err| anyhow::anyhow!("{err}"))?;
            break;
        }
        first_audio.get_or_insert_with(|| start.elapsed());
        audio += FRAME_DURATION;
    }

    Ok(Decoded {
        first_audio: first_audio.unwrap_or_default(),
        audio,
        took: start.elapsed(),
    })
}
