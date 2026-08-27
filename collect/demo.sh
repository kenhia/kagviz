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
# What it does NOT do: clear what it is about to serve. The corpus is chosen
# by the person running the demo and pre-checked by them (Ken's call, sprint
# 014) — this script selects and serves, it does not audit.
#
# Since sprint 015 it does *report* a floor: it greps the served tree for
# known credential shapes and prints what it found. That is not the same as
# auditing, and the difference is the point. A redactor's clean pass is a
# claim about the text; this is a claim about the scanner. It finds only
# shapes someone thought of, so a zero here means "none of the shapes this
# knows about", never "clean" — read the tree yourself either way.
#
#   just demo                     # kagviz's own sessions, built and served
#   just demo '*korg*' '*kmon*'   # quote the globs — your shell would eat them
#   just demo --calls             # include the call text (see below)
#   just demo --build-only        # build the tree, do not serve (pre-check)
#   just demo --serve-only        # serve the tree already built, no rebuild
#   just demo --port 9000
#   just demo-clean               # remove the tree
#
# --calls puts the tool calls' own input and result text on the demo tree, so
# the app's segment panel can open a row and show what the command was. It is
# off by default in *both* trees for the same reason: everything else kagviz
# serves is counted from the transcript, and this is the transcript. Passing
# it is the decision. See sprint 015 and docs/facts-contract.md.
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
CALLS=0
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
  just demo --calls             include each tool call's input and result text
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
    --calls) CALLS=1 ;;
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
  # --calls is passed straight through: off by default here exactly as it is
  # on the tailnet tree, and one flag away in both.
  derive_args=(derive --live "$TREE")
  [[ $CALLS -eq 1 ]] && derive_args+=(--calls)
  "$KAGVIZ" "${derive_args[@]}" || die "derive failed"

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
# --- the exposure floor ----------------------------------------------------
#
# A reporter, not a redactor, and the distinction is the whole design. It
# greps the *served* tree — the bytes the room can actually fetch — so its
# answer scales with the choice that was made: without --calls it sees the
# derived documents and their capped previews, with --calls it sees the call
# text too. What it can never say is "clean". It matches shapes someone
# thought of in an afternoon, so its zero is a fact about this scanner and
# not about the tree, and it says so in as many words.
#
# Adding a pattern here is welcome and changes nothing about that reading.

scan_label=(private-key sk-ant bearer-header KEY=value dsn-password)
scan_pattern=(
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'sk-ant-[A-Za-z0-9_-]{16,}'
  '[Aa]uthorization[^A-Za-z0-9]{1,4}[Bb]earer [A-Za-z0-9._~+/-]{16,}'
  '[A-Z0-9_]*(TOKEN|SECRET|PASSWORD|API_KEY)[A-Z0-9_]*[^A-Za-z0-9]{1,4}[A-Za-z0-9._~+/-]{12,}'
  '[a-z][a-z0-9+.-]*://[^:/@[:space:]"]+:[^@/[:space:]"]+@'
)

files=$(find "$TREE/derived" -type f \( -name '*.json' -o -name '*.html' \) \
  -not -path '*/app/*' | wc -l)
hits=()
total=0
for i in "${!scan_pattern[@]}"; do
  n=$(find "$TREE/derived" -type f \( -name '*.json' -o -name '*.html' \) \
    -not -path '*/app/*' -print0 |
    xargs -0 grep -EoIh -- "${scan_pattern[$i]}" 2>/dev/null | wc -l)
  total=$((total + n))
  [[ $n -gt 0 ]] && hits+=("${scan_label[$i]} $n")
done

echo "exposure floor"
if [[ $CALLS -eq 1 ]]; then
  echo "  CALL TEXT IS IN THIS TREE (--calls). Every tool call's input and result"
  echo "  is servable — command output, file contents, whatever was pasted in."
else
  echo "  no call text (default). The served documents are counted from the"
  echo "  transcripts; the previews on the browse page are the raw part."
fi
echo "  scanned  $files served file(s)"
if [[ $total -gt 0 ]]; then
  echo "  matched  $total:  ${hits[*]}"
else
  echo "  matched  0 of the ${#scan_pattern[@]} shapes it knows"
fi
echo "  This is a FLOOR, not a clearance: it finds only the shapes it was taught,"
echo "  so 0 means this scanner found nothing, never that there is nothing."

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
