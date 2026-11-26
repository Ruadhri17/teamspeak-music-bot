#!/bin/sh
# Installs the TeamSpeak music bot.
#
#   curl -fsSL https://raw.githubusercontent.com/Ruadhri17/teamspeak-music-bot/main/install.sh | sh
#
# Downloads the bot from the latest release, checks its SHA-256 sum and runs
# `tsmusic setup`, which asks what to install and sets up the service.
# Arguments go to setup, e.g. `sh install.sh --yes` or `sh install.sh --update`.
# TSMUSIC_VERSION=v0.2.0 picks a release other than the latest.

set -eu

repo="Ruadhri17/teamspeak-music-bot"
version="${TSMUSIC_VERSION:-latest}"

fail() {
    echo "install.sh: $*" >&2
    exit 1
}

[ "$(uname -s)" = Linux ] || fail "the bot runs on Linux only"

case "$(uname -m)" in
    x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
    aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
    *) fail "no build for $(uname -m)" ;;
esac

if [ "$version" = latest ]; then
    base="https://github.com/$repo/releases/latest/download"
else
    base="https://github.com/$repo/releases/download/$version"
fi
asset="tsmusic-$target"

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' --retry 2 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q --https-only -O "$2" "$1"
    else
        fail "curl or wget is needed to download the bot"
    fi
}

command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is needed to check the download"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $asset ($version)"
download "$base/$asset" "$tmp/$asset" || fail "cannot download $base/$asset"
download "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "cannot download $base/SHA256SUMS"

# Only the line of this asset, an empty selection must not pass as a check.
grep "  $asset\$" "$tmp/SHA256SUMS" >"$tmp/expected" || fail "SHA256SUMS does not list $asset"
(cd "$tmp" && sha256sum -c --quiet expected) || fail "the checksum of $asset does not match"
chmod 0755 "$tmp/$asset"

# Piped into sh, the standard input is this script, the questions of setup need the terminal.
if [ -t 0 ]; then
    input=/dev/stdin
elif [ -r /dev/tty ] && (: </dev/tty) 2>/dev/null; then
    input=/dev/tty
else
    input=/dev/null
fi

if [ "$(id -u)" -eq 0 ]; then
    "$tmp/$asset" setup "$@" <"$input"
elif command -v sudo >/dev/null 2>&1; then
    sudo "$tmp/$asset" setup "$@" <"$input"
else
    fail "run it as root, setup installs packages and creates a user"
fi
