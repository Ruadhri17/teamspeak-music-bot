# Project tasks. `just` lists them, `just <name>` runs one.

compose := "docker compose -f dev/docker-compose.yml"
# Keep in step with the deny job in .github/workflows/ci.yml.
deny_version := "0.22.1"

# List all tasks
default:
    @just --list

# Format check, lints, tests and cargo-deny. Run before every commit.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    cargo deny check

# A C compiler and cmake build the bundled opus, ffmpeg runs in the tests. At the end
# `tsmusic setup --tools-only` installs the missing yt-dlp and deno, without the service.
# Install the tools for working on the bot, asks for sudo
deps:
    #!/usr/bin/env sh
    set -eu
    if command -v apt-get >/dev/null; then
        sudo apt-get update -q
        sudo apt-get install -y -q build-essential cmake ffmpeg python3 curl
    elif command -v dnf >/dev/null; then
        sudo dnf install -y gcc gcc-c++ make cmake ffmpeg-free python3 curl
    elif command -v pacman >/dev/null; then
        sudo pacman -S --needed --noconfirm base-devel cmake ffmpeg python curl
    else
        echo "no apt-get, dnf or pacman found, install a C compiler, cmake, ffmpeg, python3 and curl yourself" >&2
        exit 1
    fi
    rustup component add rustfmt clippy
    cargo install --locked cargo-deny@{{deny_version}}
    cargo build
    sudo ./target/debug/tsmusic setup --tools-only --yes

# Known vulnerabilities, licenses and sources of the dependencies, see deny.toml
deny:
    cargo deny check

# Format all code
fmt:
    cargo fmt

# Run tests, e.g. `just test config::`
test *filter:
    cargo test {{filter}}

# Create config.toml in the repo root from the template
init *args:
    cargo run -- init -c config.toml {{args}}

# Run the bot with config.toml from the repo root, e.g. `just run --log-level debug`
run *args:
    cargo run -- run -c config.toml {{args}}

# Start the local TeamSpeak server
ts-up:
    {{compose}} up -d

# Stop the local TeamSpeak server, data stays in the volume
ts-down:
    {{compose}} down

# Follow the local TeamSpeak server logs
ts-logs:
    {{compose}} logs -f ts3

# Show the server admin token, printed by the server on its first start only
ts-token:
    @{{compose}} logs ts3 | grep -i token || { echo "no token in the logs: the server prints it only on its first start with an empty volume" >&2; exit 1; }
