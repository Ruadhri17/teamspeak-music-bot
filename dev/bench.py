#!/usr/bin/env python3
"""Measures the bot while it plays: CPU, memory and network of its container.

Starts the TeamSpeak server and the bot from dev/docker-compose.yml, sends the bot
`!song add` through ServerQuery, samples `docker stats` for the given time and prints
a summary. The samples go to target/bench/<time>.csv, the audio statistics of the bot
(logging.metrics_interval) are printed from its log.

    just bench                       # 5 minutes of local test tracks (pink noise)
    just bench --minutes 30
    just bench --youtube 'https://www.youtube.com/playlist?list=...'

The container holds the bot with ffmpeg, yt-dlp and deno. Its totals come from
`docker stats`, and every sample also splits CPU and memory by program, read from /proc
inside the container. CPU is in percent of one core. The network splits by itself: the
upload is the voice to the server, the download is the stream from YouTube. Memory per
program is its RSS, which counts the shared libraries it maps (ffmpeg maps many), the
container total from the kernel counts them once and is the one to compare.
"""

import argparse
import csv
import re
import socket
import statistics
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COMPOSE = ["docker", "compose", "-f", str(ROOT / "dev" / "docker-compose.yml"), "--profile", "bot"]
CONTAINER = "tsmusic-dev-bot"
BOT_NICKNAME = "MusicBot"
QUERY = ("127.0.0.1", 10011)
QUERY_PASSWORD = "dev-only-password"
LOCAL_TRACKS = ["noise1.mp3", "noise2.mp3", "noise3.mp3"]
PROGRAMS = ["bot", "ffmpeg", "yt-dlp", "deno"]

# Runs inside the container: the exact bytes of its network, then per process the
# program, its CPU seconds and its memory. `docker stats` rounds the network to three
# digits, too coarse for a sample every two seconds. yt-dlp is a Python zipapp, it is
# told apart from this script by its command line.
PROCESSES = r"""
import os
rx = tx = 0
for line in open("/proc/net/dev").read().splitlines()[2:]:
    name, counters = line.split(":", 1)
    if name.strip() != "lo":
        fields = counters.split()
        rx, tx = rx + int(fields[0]), tx + int(fields[8])
print("net", rx, tx)
hz, page, me = os.sysconf("SC_CLK_TCK"), os.sysconf("SC_PAGE_SIZE"), os.getpid()
for pid in filter(str.isdigit, os.listdir("/proc")):
    if int(pid) == me:
        continue
    try:
        args = open(f"/proc/{pid}/cmdline", "rb").read().split(b"\0")
        fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        rss = int(open(f"/proc/{pid}/statm").read().split()[1]) * page
    except OSError:
        continue
    command = os.path.basename(args[0].decode())
    if command == "tsmusic":
        name = "bot"
    elif command in ("ffmpeg", "deno"):
        name = command
    elif any(b"yt-dlp" in arg for arg in args):
        name = "yt-dlp"
    else:
        continue
    print(pid, name, (int(fields[11]) + int(fields[12])) / hz, rss)
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--minutes", type=float, default=5, help="how long to measure")
    parser.add_argument("--youtube", help="a YouTube link to play instead of the test tracks")
    args = parser.parse_args()

    print("starting the server and the bot (the first build takes a few minutes)")
    subprocess.run([*COMPOSE, "up", "-d", "--build", "ts3", "bot"], check=True)

    query = connect_query(timeout=60)
    query.login()
    bot = query.wait_for(BOT_NICKNAME, timeout=120)
    query.let_the_bot_see_queries()
    query.join(bot["cid"])
    # A run that was stopped may have left songs behind.
    query.say("!queue clear")
    commands = [f"!song add {args.youtube}"] if args.youtube else [
        f"!song add --source local {track}" for track in LOCAL_TRACKS
    ]
    for command in commands:
        query.say(command)
    started = datetime.now().astimezone()
    print(f"playing: {', '.join(commands)}")

    samples = sample(args.minutes * 60, query)
    query.say("!queue clear")
    query.close()

    out = ROOT / "target" / "bench" / f"{started:%Y-%m-%d_%H-%M-%S}.csv"
    write_csv(out, samples)
    summarize(samples)
    print_metrics(started)
    print(f"samples: {out}")
    print("the bot keeps running, `just bot-down` stops it")


def connect_query(timeout):
    """Docker accepts the port before the server listens on it, and a server that was
    just recreated takes a few seconds: retried until the server greets."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            return Query(*QUERY)
        except OSError as err:
            if time.monotonic() > deadline:
                sys.exit(f"ServerQuery at {QUERY[0]}:{QUERY[1]} does not answer: {err}")
            time.sleep(1)


class Query:
    """A ServerQuery client: one command per line, answers end with an `error` line."""

    def __init__(self, host, port):
        self.sock = socket.create_connection((host, port), timeout=10)
        self.buffer = b""
        try:
            self.read_line()  # "TS3"
            self.read_line()  # the welcome text
        except OSError:
            self.sock.close()
            raise

    def read_line(self):
        while b"\n" not in self.buffer:
            data = self.sock.recv(4096)
            if not data:
                raise ConnectionError("ServerQuery closed the connection")
            self.buffer += data
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode().strip()

    def command(self, text):
        self.sock.sendall(text.encode() + b"\n")
        lines = []
        while True:
            line = self.read_line()
            if line.startswith("error "):
                fields = parse(line)[0]
                if fields.get("id") != "0":
                    raise RuntimeError(f"{text.split()[0]}: {unescape(fields.get('msg', ''))}")
                return parse(lines[0]) if lines else []
            if line:
                lines.append(line)

    def login(self):
        self.command(f"login serveradmin {QUERY_PASSWORD}")
        self.command("use sid=1")
        self.command("clientupdate client_nickname=bench")

    def wait_for(self, nickname, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for client in self.command("clientlist"):
                if client.get("client_nickname") == nickname and client.get("client_type") == "0":
                    return client
            time.sleep(2)
        sys.exit(f"{nickname} did not connect in {timeout} s, see `docker logs {CONTAINER}`")

    def let_the_bot_see_queries(self):
        """Query clients are hidden from normal clients, the bot would not know who wrote
        to it. The default server group, the bot's, may see them on this dev server."""
        group = self.command("serverinfo")[0]["virtualserver_default_server_group"]
        self.command(
            f"servergroupaddperm sgid={group} permsid=i_client_serverquery_view_power "
            "permvalue=100 permnegated=0 permskip=0"
        )

    def join(self, channel):
        me = self.command("whoami")[0]
        if me["client_channel_id"] != channel:
            self.command(f"clientmove clid={me['client_id']} cid={channel}")
        self.channel = channel

    def say(self, text):
        """A message in the channel the query client is in, the bot's."""
        self.command(f"sendtextmessage targetmode=2 target={self.channel} msg={escape(text)}")

    def keep_alive(self):
        self.command("whoami")

    def close(self):
        try:
            self.command("quit")
        except (ConnectionError, OSError):
            pass
        self.sock.close()


ESCAPES = [("\\", "\\\\"), ("/", "\\/"), (" ", "\\s"), ("|", "\\p"), ("\n", "\\n")]


def escape(text):
    for plain, escaped in ESCAPES:
        text = text.replace(plain, escaped)
    return text


def unescape(text):
    for plain, escaped in reversed(ESCAPES):
        text = text.replace(escaped, plain)
    return text


def parse(line):
    """`a=1 b=x|a=2 b=y` into a list of dicts."""
    entries = []
    for entry in line.split("|"):
        fields = {}
        for word in entry.split(" "):
            key, _, value = word.partition("=")
            fields[key] = unescape(value)
        entries.append(fields)
    return entries


def sample(seconds, query):
    """One row per `docker stats` call, about every two seconds."""
    samples = []
    previous = None
    start = time.monotonic()
    last_keep_alive = start
    while time.monotonic() - start < seconds:
        line = subprocess.run(
            ["docker", "stats", "--no-stream", "--format",
             "{{.CPUPerc}};{{.MemUsage}}", CONTAINER],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        (rx, tx), processes = read_processes()
        now = time.monotonic()
        cpu, memory = line.split(";")
        if previous is not None:
            elapsed = now - previous[0]
            row = {
                "seconds": round(now - start, 1),
                "cpu_percent": float(cpu.rstrip("%")),
                "memory_mib": to_bytes(memory.split("/")[0]) / 2**20,
                "download_kbit_s": (rx - previous[1]) * 8 / 1000 / elapsed,
                "upload_kbit_s": (tx - previous[2]) * 8 / 1000 / elapsed,
            }
            row.update(per_program(processes, previous[3], elapsed))
            samples.append(row)
        previous = (now, rx, tx, processes)
        if now - last_keep_alive > 60:
            query.keep_alive()
            last_keep_alive = now
    return samples


def read_processes():
    """The bytes received and sent so far, and pid: (program, CPU seconds so far,
    memory in bytes)."""
    output = subprocess.run(
        ["docker", "exec", CONTAINER, "python3", "-c", PROCESSES],
        capture_output=True, text=True, check=True,
    ).stdout
    net = (0, 0)
    processes = {}
    for line in output.splitlines():
        words = line.split()
        if words[0] == "net":
            net = (int(words[1]), int(words[2]))
            continue
        pid, name, cpu, rss = words
        processes[pid] = (name, float(cpu), int(rss))
    return net, processes


def per_program(processes, before, elapsed):
    """CPU in percent of one core over the interval and memory now, summed per program.
    A process that started in the interval counts from zero."""
    row = {}
    for program in PROGRAMS:
        cpu = sum(
            seconds - before.get(pid, (name, 0.0, 0))[1]
            for pid, (name, seconds, _) in processes.items()
            if name == program
        )
        memory = sum(rss for name, _, rss in processes.values() if name == program)
        row[f"{program}_cpu_percent"] = max(cpu, 0.0) * 100 / elapsed
        row[f"{program}_memory_mib"] = memory / 2**20
    return row


UNITS = {"B": 1, "kB": 1e3, "KB": 1e3, "MB": 1e6, "GB": 1e9, "TB": 1e12,
         "KiB": 2**10, "MiB": 2**20, "GiB": 2**30, "TiB": 2**40}


def to_bytes(text):
    match = re.fullmatch(r"\s*([\d.]+)\s*([A-Za-z]+)\s*", text)
    if not match:
        raise ValueError(f"not a size: {text!r}")
    return float(match.group(1)) * UNITS[match.group(2)]


def write_csv(path, samples):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=list(samples[0]))
        writer.writeheader()
        writer.writerows(samples)


def summarize(samples):
    rows = [("container", None)]
    rows += [(label, key) for key, label in [
        ("cpu_percent", "CPU %"), ("memory_mib", "memory MiB"),
        ("download_kbit_s", "down kbit/s"), ("upload_kbit_s", "up kbit/s"),
    ]]
    for program in PROGRAMS:
        rows += [(program, None), ("CPU %", f"{program}_cpu_percent"),
                 ("memory MiB", f"{program}_memory_mib")]
    print(f"\n{len(samples)} samples, memory per program includes its shared libraries")
    print(f"{'':16}{'average':>10}{'p95':>10}{'max':>10}")
    for label, key in rows:
        if key is None:
            print(label)
            continue
        values = sorted(sample[key] for sample in samples)
        p95 = values[min(len(values) - 1, len(values) * 95 // 100)]
        print(f"  {label:14}{statistics.mean(values):10.1f}{p95:10.1f}{values[-1]:10.1f}")


def print_metrics(started):
    log = subprocess.run(
        ["docker", "logs", "--since", started.isoformat(), CONTAINER],
        capture_output=True, text=True,
    )
    lines = [line for line in (log.stdout + log.stderr).splitlines() if "audio:" in line]
    print("\nthe bot's audio statistics:")
    for line in lines or ["(none, the bot logs them every logging.metrics_interval)"]:
        print(f"  {line}")


if __name__ == "__main__":
    main()
