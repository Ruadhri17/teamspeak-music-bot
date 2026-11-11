# Project tasks. `just` lists them, `just <name>` runs one.

compose := "docker compose -f dev/docker-compose.yml"

# List all tasks
default:
    @just --list

# TODO: add `-- -D warnings` to clippy once the client TODOs remove the dead code warnings.

# Format check, lints and tests. Run before every commit.
check:
    cargo fmt --check
    cargo clippy --all-targets
    cargo test

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
