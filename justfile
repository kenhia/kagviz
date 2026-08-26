# Windows: `just` runs recipes through `sh`, which Windows does not ship — put
# Git for Windows' `usr\bin` on PATH (it holds `sh.exe`) or run from Git Bash.
# (Upstream's own requirement: "sh must be available in the PATH".)

# List available recipes
default:
    @just --list

# The served tree the collector writes and copyparty serves (docs/collection.md)
kagviz_live := env_var_or_default("KAGVIZ_LIVE", "/ai-data/kagviz-data/live")

# Run CI gates — Rust and the app. A gate that skips the app is a gate that lies.
check: rust-check web-check

# Run the Rust gates alone
rust-check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Apply formatting
fmt:
    cargo fmt
    cd web && npm run format

# --- the app: web/ (sprint 011) ---

# Install the app's dependencies from the lockfile
web-install:
    cd web && npm ci

# The vitest run includes the contract conformance test over tests/golden/,
# which is why a facts change that breaks the app fails the build on the Rust
# side too, the day it lands.

# The app's gates: prettier/eslint, svelte-check, build, vitest
web-check:
    cd web && ([ -d node_modules ] || npm ci)
    cd web && npm run lint
    cd web && npm run check
    cd web && npm run build
    cd web && npm test

# There is no derived tree beside the dev server, so point the app at a served
# one:  VITE_KAGVIZ_DERIVED=https://kai.encke-wahoo.ts.net:8027/kagviz/ just web-dev

# The app's dev server
web-dev:
    cd web && npm run dev

# derived/app/ puts the bundle on the same origin as the data it reads — no
# CORS, no k-homelab manifest change — and the tree stays regenerable, which is
# the rule for everything under derived/. Staged and renamed, so copyparty
# never serves a half-copied bundle.

# Build the app and install it at derived/app/ on the served tree
web-deploy:
    cd web && ([ -d node_modules ] || npm ci)
    cd web && npm run build
    rm -rf "{{kagviz_live}}/derived/app.new" "{{kagviz_live}}/derived/app.old"
    cp -r web/build "{{kagviz_live}}/derived/app.new"
    if [ -d "{{kagviz_live}}/derived/app" ]; then mv "{{kagviz_live}}/derived/app" "{{kagviz_live}}/derived/app.old"; fi
    mv "{{kagviz_live}}/derived/app.new" "{{kagviz_live}}/derived/app"
    rm -rf "{{kagviz_live}}/derived/app.old"
    @echo "app → {{kagviz_live}}/derived/app  (open /kagviz/app/index.html)"

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
