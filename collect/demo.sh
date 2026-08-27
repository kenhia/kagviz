#!/usr/bin/env bash
# kagviz collect/demo.sh — build a curated tree and serve it where a machine
# that is not on the tailnet can reach it. `just demo`.
#
# Why this exists at all: copyparty binds 127.0.0.1:8027, and the listeners on
# that port on kai's tailnet IP belong to `tailscaled` (`tailscale serve`
# terminating TLS and proxying to loopback). There is **no LAN path** to
# kagviz. `kwork` is Microsoft-managed and deliberately off the tailnet but
# reaches the LAN, so showing kagviz over Teams needs a second, temporary
# server on kai's LAN address. That is the whole job.
#
# What it does NOT do: check what it is about to serve. The corpus is chosen
# by the person running the demo and pre-checked by them (Ken's call, sprint
# 014) — this script selects and serves, it does not audit. It prints what is
# in the tree so the presenter can see what the room will see, and stops
# there.
#
#   just demo                     # kagviz's own sessions, built and served
#   just demo '*korg*' '*kmon*'   # quote the globs — your shell would eat them
#   just demo --build-only        # build the tree, do not serve (pre-check)
#   just demo --serve-only        # serve the tree already built, no rebuild
#   just demo --port 9000
#   just demo-clean               # remove the tree
#
# Env: KAGVIZ_LIVE (mirror root), KAGVIZ_BIN (default: this repo's release
# build), KAGVIZ_DEMO_TREE (where the curated tree goes),
# KAGVIZ_DEMO_ADDR (bind address, if the default route resolves wrong).
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(dirname "$HERE")
LIVE="${KAGVIZ_LIVE:-/ai-data/kagviz-data/live}"
KAGVIZ="${KAGVIZ_BIN:-$REPO/target/release/kagviz}"
TREE="${KAGVIZ_DEMO_TREE:-$HOME/.cache/kagviz-demo}"
PORT=8028
BUILD=1
SERVE=1
PATTERNS=()

die() {
  echo "demo: $*" >&2
  exit 2
}

usage() {
  cat <<'EOF'
just demo [OPTIONS] [PROJECT-GLOB...]

Build a curated kagviz tree out of the live mirror and serve it on this host's
LAN address, where a machine that is not on the tailnet can reach it.

  just demo                     kagviz's own sessions, built and served
  just demo '*korg*' '*kmon*'   quote the globs — your shell would eat them
  just demo --build-only        build the tree, do not serve (pre-check)
  just demo --serve-only        serve the tree already built, no rebuild
  just demo --port 9000         default is 8028
  just demo-clean               remove the tree

Env: KAGVIZ_LIVE, KAGVIZ_BIN, KAGVIZ_DEMO_TREE, KAGVIZ_DEMO_ADDR.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-only) SERVE=0 ;;
    --serve-only) BUILD=0 ;;
    --port)
      [[ $# -ge 2 ]] || die "--port needs a number"
      PORT="$2"
      shift
      ;;
    --port=*) PORT="${1#*=}" ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*) die "unknown option $1" ;;
    *) PATTERNS+=("$1") ;;
  esac
  shift
done

# kagviz's own sessions are the right default: a public repo, no other
# project's paths in the tree, and the tool showing the sessions that built it.
[[ ${#PATTERNS[@]} -gt 0 ]] || PATTERNS=('*kagviz*')

command -v python3 >/dev/null || die "python3 is not installed (it is the server)"
command -v jq >/dev/null || die "jq is not installed (it reads the summary back)"

# --- build ----------------------------------------------------------------

if [[ $BUILD -eq 1 ]]; then
  [[ -x "$KAGVIZ" ]] || die "$KAGVIZ is not built — run \`cargo build --release\`"
  [[ -d "$LIVE" ]] || die "$LIVE does not exist — is this kai?"

  # A fresh tree every time. It is derived from the mirror and regenerable,
  # and a stale session left over from the last demo is exactly the kind of
  # thing nobody notices until it is on a projector.
  rm -rf "$TREE"
  mkdir -p "$TREE"

  shopt -s nullglob nocasematch
  copied=()
  for projects in "$LIVE"/*/projects; do
    host=$(basename "$(dirname "$projects")")
    for dir in "$projects"/*; do
      [[ -d "$dir" ]] || continue
      name=$(basename "$dir")
      for pat in "${PATTERNS[@]}"; do
        if [[ $name == $pat ]]; then
          mkdir -p "$TREE/$host/projects"
          cp -a "$dir" "$TREE/$host/projects/$name"
          copied+=("$host:$name")
          break
        fi
      done
    done
  done
  shopt -u nullglob nocasematch

  if [[ ${#copied[@]} -eq 0 ]]; then
    echo "demo: no project directory in $LIVE matched: ${PATTERNS[*]}" >&2
    echo "available:" >&2
    for projects in "$LIVE"/*/projects; do
      host=$(basename "$(dirname "$projects")")
      for dir in "$projects"/*; do
        [[ -d "$dir" ]] && echo "  $host:$(basename "$dir")" >&2
      done
    done
    # Leave nothing behind: a half-built tree here would make the next
    # --serve-only fail with a stranger message than this one.
    rm -rf "$TREE"
    exit 2
  fi

  # sync-status.json is deliberately NOT copied. It reports the collector's
  # last run over the whole fleet, which says nothing true about a hand-picked
  # tree — and the line the index prints without it ("the collector has not
  # run, or these mirrors were not written by it") is exactly right here.
  "$KAGVIZ" derive --live "$TREE" || die "derive failed"

  # Quiet unless it breaks: the npm build's output is a screenful, and the one
  # line web-deploy prints names copyparty's mount rather than this one.
  echo "demo: building the app..."
  build_log=$(mktemp)
  if ! KAGVIZ_LIVE="$TREE" just --justfile "$REPO/justfile" web-deploy >"$build_log" 2>&1; then
    cat "$build_log" >&2
    rm -f "$build_log"
    die "web-deploy failed"
  fi
  rm -f "$build_log"

  # derive writes index.html before the app exists, so the "Open the app" link
  # is absent from the page it just wrote. Regenerate the index now that
  # derived/app/ is there — otherwise the demo's browse page has no way into
  # the app, which is the half a demo is actually for.
  "$KAGVIZ" index "$TREE/derived" || die "index failed"
fi

[[ -d "$TREE/derived" ]] || die "$TREE/derived does not exist — run without --serve-only first"

# --- what is in it --------------------------------------------------------

sessions="$TREE/derived/sessions.json"
total=$(jq '.sessions | length' "$sessions")
per_host=$(jq -r '[.sessions[].host] | group_by(.) | map("\(.[0]) \(length)") | join(", ")' "$sessions")
# The served size is derived/ alone; the copied mirror beside it never goes on
# the wire. Report both, because one of them is what the audience downloads and
# the other is what filled the disk.
served=$(du -sh "$TREE/derived" | cut -f1)
size=$(du -sh "$TREE" | cut -f1)

echo
echo "demo tree  $TREE"
echo "  corpus   ${PATTERNS[*]}"
if [[ $BUILD -eq 1 ]]; then
  printf '           %s\n' "${copied[@]}"
fi
echo "  sessions $total  ($per_host)"
echo "  size     $served served  ($size on disk, with the copied mirror)"
echo
echo "This is what the room will see. The prompt previews on the browse page are"
echo "the user's own words; the cwd paths and branch names are real. Look before"
echo "you share the screen — nothing here checks it for you."

[[ $SERVE -eq 1 ]] || {
  echo
  echo "Built, not served. \`just demo --serve-only\` puts it on the LAN."
  exit 0
}

# --- serve ----------------------------------------------------------------

# The demo host's address is not a constant, and a wrong one fails silently as
# "connection refused" while someone is sharing their screen. Take it from the
# default route rather than hardcoding an interface name — tailscale's routes
# are per-host /32s and never the default, so this cannot pick the tailnet IP
# by accident.
ADDR="${KAGVIZ_DEMO_ADDR:-}"
if [[ -z $ADDR ]]; then
  ADDR=$(ip -4 route show default 2>/dev/null | sed -n 's/.*[[:space:]]src[[:space:]]\+\([0-9.]\+\).*/\1/p' | head -1)
fi
[[ -n $ADDR ]] || ADDR=$(hostname -I 2>/dev/null | awk '{print $1}')
[[ -n $ADDR ]] || die "could not resolve a LAN address — set KAGVIZ_DEMO_ADDR"

case "$ADDR" in
  100.6[4-9].* | 100.[7-9][0-9].* | 100.1[01][0-9].* | 100.12[0-7].*)
    echo
    echo "demo: $ADDR is in 100.64.0.0/10 — that looks like a tailnet address," >&2
    echo "      which is the one place a demo does not need a second server." >&2
    echo "      Set KAGVIZ_DEMO_ADDR to the LAN address if this is wrong." >&2
    ;;
esac

echo
echo "serving   http://$ADDR:$PORT/index.html      the browse page"
echo "          http://$ADDR:$PORT/app/index.html  the app"
echo
echo "Plaintext HTTP on the LAN — no TLS, no accounts. Anyone who can reach"
echo "$ADDR can read every session in this tree. That is the trade for reaching"
echo "a machine that is not on the tailnet; it is a choice, not an oversight."
echo
echo "Ctrl-C stops the server. It does not outlive this terminal."
echo

# Not `exec`: the trap is the teardown notice, and an exec'd process has no
# shell left to run it in.
trap 'echo; echo "demo: server stopped. The tree is still at $TREE (\`just demo-clean\` removes it)."' EXIT

python3 -m http.server "$PORT" --bind "$ADDR" --directory "$TREE/derived"
