#!/usr/bin/env bash
# kagviz collect/collect.sh — the nightly entry point, and `just collect`.
#
# Sync every host, then derive. Derive runs even when a host was unreachable
# or the sync reported a failure: the index has to say what arrived, not wait
# for everything to. The exit status is the worse of the two stages, so the
# systemd unit reads `failed` when a person should look and `ok` when cleo was
# merely asleep.
#
# Arguments are passed through to `kagviz derive` (e.g. `--label`, `--force`).
# Env: KAGVIZ_LIVE (mirror root), KAGVIZ_BIN (default: this repo's release build)
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(dirname "$HERE")
LIVE="${KAGVIZ_LIVE:-/ai-data/kagviz-data/live}"
KAGVIZ="${KAGVIZ_BIN:-$REPO/target/release/kagviz}"

[[ -x "$KAGVIZ" ]] || {
  echo "collect: $KAGVIZ is not built — run \`just collect-install\` (or \`cargo build --release\`)" >&2
  exit 2
}

"$HERE/sync.sh"
sync_rc=$?

"$KAGVIZ" derive --live "$LIVE" "$@"
derive_rc=$?

[[ $derive_rc -ne 0 ]] && exit "$derive_rc"
exit "$sync_rc"
