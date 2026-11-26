# syntax=docker/dockerfile:1
# The bot for dev/docker-compose.yml, for local tests and `just bench`. Not published.
# Built from the repository root: docker build -f dev/bot.Dockerfile .

FROM rust:1-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# The caches stay between builds, a change rebuilds only the bot.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/tsmusic /tsmusic

FROM debian:trixie-slim
COPY --from=build /tsmusic /usr/local/bin/tsmusic
# ffmpeg, yt-dlp and deno the way a server gets them, which also tests setup.
RUN tsmusic setup --tools-only --yes && rm -rf /var/lib/apt/lists/*
# Tracks for `just bench`: pink noise, a harder case for Opus than most music.
RUN mkdir -p /bot/music \
    && for n in 1 2 3; do \
        ffmpeg -loglevel error -f lavfi -i "anoisesrc=color=pink:amplitude=0.2:duration=300" \
            -ac 2 -b:a 192k "/bot/music/noise$n.mp3"; \
    done
RUN useradd --system --home-dir /bot tsmusic \
    && mkdir -p /bot/state \
    && chown tsmusic:tsmusic /bot/state
USER tsmusic
WORKDIR /bot
ENTRYPOINT ["tsmusic", "--config", "/bot/config.toml", "run"]
