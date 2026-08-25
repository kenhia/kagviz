#!/usr/bin/env bash
# kagviz collect/sync.sh — pull every host's ~/.claude/projects into the live
# mirror under $KAGVIZ_LIVE/<host>/projects/.
#
# Mirror, never prune. The CLI deletes transcripts after ~30 days; the mirror
# is where they survive. New and updated files are copied, and a deletion at
# the source is never propagated (no --delete anywhere in this file, on
# purpose). `.kagviz/` is the one thing excluded: it is kagviz's own label
# cache, not something the harness wrote.
#
# An unreachable host is a normal night, not a failure. cleo sleeps and
# Windows Update reboots it; kubs0 has maintenance windows. Every host is tried
# independently, one that does not answer is recorded as `unreachable` in
# sync-status.json, and the run carries on. The exit status is non-zero only
# for something a person should look at — a host that answered and then failed
# mid-sync, or a missing tool — never for a host that was asleep.
#
# Per host:
#   kai    local rsync
#   kubs0  rsync over ssh
#   cleo   rclone copy over sftp (Windows: no rsync; rclone does size+mtime
#          incrementals and writes each file to a temp name then renames, so a
#          reader never sees a half-copied transcript). rclone does not read
#          ~/.ssh/config, so the host/user/key are read out of `ssh -G cleo`
#          rather than duplicated here.
#
# Usage: collect/sync.sh [host...]          default: kai kubs0 cleo
# Env:   KAGVIZ_LIVE           mirror root (default /ai-data/kagviz-data/live)
#        KAGVIZ_SYNC_TIMEOUT   per-host cap, `timeout` syntax (default 30m)
set -uo pipefail

LIVE="${KAGVIZ_LIVE:-/ai-data/kagviz-data/live}"
TIMEOUT="${KAGVIZ_SYNC_TIMEOUT:-30m}"
HOSTS=("$@")
[[ ${#HOSTS[@]} -gt 0 ]] || HOSTS=(kai kubs0 cleo)
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

mkdir -p "$LIVE" || { echo "sync: cannot create $LIVE" >&2; exit 2; }
STATUS="$LIVE/sync-status.json"
LOG="$LIVE/sync.log"
RAN_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)

declare -A RESULT FILES SECS NOTE
worst=0

record() { # host status files secs note
  RESULT[$1]=$2; FILES[$1]=$3; SECS[$1]=$4; NOTE[$1]=${5:-}
  printf '%-6s %-12s %5s file(s) %4ss  %s\n' "$1" "$2" "$3" "$4" "${5:-}"
  printf '%s %s %s %s files %ss %s\n' "$RAN_AT" "$1" "$2" "$3" "$4" "${5:-}" >> "$LOG"
  [[ $2 == failed ]] && worst=1
  return 0
}

reachable() { # host — one cheap ssh, so "asleep" is classified before any transfer starts
  ssh -n "${SSH_OPTS[@]}" "$1" exit >/dev/null 2>&1
}

# The ssh identity kai uses for a host, read from ssh's own resolution so
# ~/.ssh/config stays the single source of truth.
ssh_field() { ssh -G "$1" 2>/dev/null | awk -v k="$2" '$1 == k { print $2; exit }'; }
ssh_key() {
  local k
  while read -r _ k; do
    k=${k/#\~/$HOME}
    [[ -f "$k" ]] && { echo "$k"; return 0; }
  done < <(ssh -G "$1" 2>/dev/null | awk '$1 == "identityfile"')
  return 1
}

# rsync's exit codes, classified. 24 is "a source file vanished mid-transfer",
# which is exactly what a self-pruning source does and is not an error here.
rsync_result() { # host rc output
  local host=$1 rc=$2 out=$3 n
  n=$(grep -c '^>f' <<< "$out")
  case "$rc" in
    0|24) record "$host" ok "$n" "$((SECONDS - t0))" "$([[ $rc == 24 ]] && echo 'some source files vanished mid-transfer (source prunes itself)')" ;;
    124)  record "$host" failed "$n" "$((SECONDS - t0))" "timed out after $TIMEOUT" ;;
    255|10|12|30|35) record "$host" unreachable "$n" "$((SECONDS - t0))" "lost the connection mid-sync (rsync exit $rc)" ;;
    *)    record "$host" failed "$n" "$((SECONDS - t0))" "rsync exit $rc: $(tail -n 1 <<< "$out")" ;;
  esac
}

sync_host() {
  local host=$1 dest="$LIVE/$1/projects" out rc t0=$SECONDS
  mkdir -p "$dest"
  case "$host" in
    kai)
      out=$(timeout "$TIMEOUT" rsync -ai --no-g --exclude '.kagviz/' "$HOME/.claude/projects/" "$dest/" 2>&1); rc=$?
      rsync_result "$host" "$rc" "$out" ;;
    kubs0)
      reachable "$host" || { record "$host" unreachable 0 "$((SECONDS - t0))" "did not answer ssh"; return; }
      out=$(timeout "$TIMEOUT" rsync -ai --no-g --exclude '.kagviz/' -e "ssh ${SSH_OPTS[*]}" "$host:.claude/projects/" "$dest/" 2>&1); rc=$?
      rsync_result "$host" "$rc" "$out" ;;
    cleo)
      reachable "$host" || { record "$host" unreachable 0 "$((SECONDS - t0))" "did not answer ssh"; return; }
      command -v rclone >/dev/null || { record "$host" failed 0 0 "rclone is not installed on this host (docs/collection.md)"; return; }
      local h u k
      h=$(ssh_field "$host" hostname); u=$(ssh_field "$host" user); k=$(ssh_key "$host") \
        || { record "$host" failed 0 0 "no usable identity file for $host in ssh -G"; return; }
      # shell_type=none: rclone's shell probe assumes a POSIX or PowerShell shell
      # it can run commands in; a plain copy needs neither.
      local remote=":sftp,host=$h,user=$u,key_file=$k,known_hosts_file=$HOME/.ssh/known_hosts,shell_type=none:.claude/projects"
      out=$(timeout "$TIMEOUT" rclone copy -v --stats 0 --exclude '.kagviz/**' "$remote" "$dest/" 2>&1); rc=$?
      local n; n=$(grep -c ': Copied' <<< "$out")
      case "$rc" in
        0)   record "$host" ok "$n" "$((SECONDS - t0))" ;;
        124) record "$host" failed "$n" "$((SECONDS - t0))" "timed out after $TIMEOUT" ;;
        *)   record "$host" failed "$n" "$((SECONDS - t0))" "rclone exit $rc: $(grep -E 'ERROR|Failed' <<< "$out" | tail -n 1)" ;;
      esac ;;
    *)
      record "$host" failed 0 0 "no sync method for host $host (this script knows kai, kubs0, cleo)" ;;
  esac
}

echo "sync $RAN_AT → $LIVE"
for host in "${HOSTS[@]}"; do
  sync_host "$host"
done

# The status file is what makes an absence visible: `kagviz derive` copies it
# into derived/ and the index page shows which hosts were reached.
json=$(jq -n --arg ran_at "$RAN_AT" '{ran_at: $ran_at, hosts: {}}')
for host in "${HOSTS[@]}"; do
  json=$(jq --arg h "$host" --arg s "${RESULT[$host]}" --argjson f "${FILES[$host]}" \
            --argjson t "${SECS[$host]}" --arg n "${NOTE[$host]}" \
            '.hosts[$h] = {status: $s, transferred: $f, secs: $t} + (if $n != "" then {note: $n} else {} end)' \
            <<< "$json")
done
printf '%s\n' "$json" > "$STATUS.tmp" && mv "$STATUS.tmp" "$STATUS"
exit $worst
