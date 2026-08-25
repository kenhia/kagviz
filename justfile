# Windows: `just` runs recipes through `sh`, which Windows does not ship — put
# Git for Windows' `usr\bin` on PATH (it holds `sh.exe`) or run from Git Bash.
# (Upstream's own requirement: "sh must be available in the PATH".)

# List available recipes
default:
    @just --list

# Run CI gates (lint, typecheck, tests)
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Apply formatting
fmt:
    cargo fmt

# --- collection: the live mirror and the nightly derive (docs/collection.md) ---

# Sync every host's transcripts into the live mirror, then derive facts, reports and the index
collect *args: build-release
    collect/collect.sh {{args}}

# Sync only — pull the mirrors, no derive (optionally: just collect-sync cleo)
collect-sync *hosts:
    collect/sync.sh {{hosts}}

# Derive only — facts, reports and the index over what is already mirrored
collect-derive *args: build-release
    target/release/kagviz derive {{args}}

# The release build the collector runs (the timer runs this checkout's binary)
build-release:
    cargo build --release

# Install + enable the 04:00 timer on this host (kmon pattern; units in collect/)
collect-install: build-release
    mkdir -p ~/.config/systemd/user
    cp collect/kagviz-collect.service collect/kagviz-collect.timer ~/.config/systemd/user/
    systemctl --user daemon-reload
    systemctl --user enable --now kagviz-collect.timer
    @systemctl --user list-timers kagviz-collect.timer --no-pager

# Timer, last run, and which hosts the last sync reached
collect-status:
    systemctl --user list-timers kagviz-collect.timer --no-pager
    systemctl --user status kagviz-collect.service --no-pager -n 20 || true
    @cat "${KAGVIZ_LIVE:-/ai-data/kagviz-data/live}/sync-status.json" 2>/dev/null || echo "no sync-status.json yet"

# Run the scheduled unit once, right now (exactly as the timer would)
collect-run:
    systemctl --user start kagviz-collect.service
    journalctl --user -u kagviz-collect.service -n 40 --no-pager
