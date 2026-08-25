# Collection — the live mirror and the nightly derive

Claude Code prunes `~/.claude/projects` on roughly a 30-day window. A session
vanished from under a corpus sweep *while it was running* (sprint 003), which
settles the question: without collection there is eventually nothing to
browse. Collection is what makes session history exist.

Everything here lives on **kai**, which owns `/ai-data` (local NVMe). The
pinned `corpus/` and `baselines/` beside it are a different thing — immutable
snapshots with a date and a commit on them — and are untouched by any of this.

## Layout

```
/ai-data/kagviz-data/live/
    kai/projects/            # verbatim mirror of that host's ~/.claude/projects
    kubs0/projects/
    cleo/projects/
    sync-status.json         # which hosts the last sync reached
    sync.log                 # one line per host per run, appended forever
    derived/                 # everything computed from the mirrors
        facts/<host>/<session-id>.json     # `kagviz show --json`, byte for byte
        reports/<host>/<session-id>.html   # `kagviz render`
        sessions.json        # the cross-host index — a contract (facts-contract.md)
        index.html           # the page a person picks a session from
        state.json           # per session: source digest + the kagviz that derived it
        META.json            # the last run: kagviz version, when, per-host counts
        sync-status.json     # copied from above, so the served tree carries it
        labels/              # the label cache, when --label is used
```

Two rules, both inherited from the pinned store:

- **Mirror, never prune.** The source deletes after ~30 days; the mirror is
  where history survives. A sync copies new and updated files and **never
  propagates a deletion** — there is no `--delete` anywhere in `collect/`, on
  purpose. `.kagviz/` (kagviz's own label cache, if someone ran `--label` on a
  live root) is the one thing excluded, because the harness did not write it.
- **Verbatim raw, everything else derived.** `live/<host>/projects` is
  transcript bytes exactly as written, sidecars and `tool-results/` included.
  Nothing under kagviz ever writes there. Anything computed goes under
  `derived/`, stamped with the kagviz that produced it, and is regenerable at
  will — delete the whole directory and the next `derive` rebuilds it.

## The sync — `collect/sync.sh`

One host at a time, each independently:

| host | how | why |
|---|---|---|
| kai | local `rsync -a --no-g` | it is the same machine |
| kubs0 | `rsync -a --no-g` over ssh | rsync on both ends |
| cleo | `rclone copy` over sftp | Windows: no rsync. rclone does size+mtime incrementals, never deletes, and writes each file to a temp name then renames, so a reader never sees a half-copied transcript. It does not read `~/.ssh/config`, so the host, user and key are read out of `ssh -G cleo` at run time rather than duplicated. `shell_type=none` skips rclone's shell probe, which assumes a POSIX or PowerShell shell it can run commands in; a copy needs neither. |

`--no-g` matters: `rsync -a` alone re-preserves the source group and defeats
the volume's setgid `ai` group (the pinned store's README records the same).

**An unreachable host is a normal night, not a failure.** cleo sleeps, and
Windows Update reboots it on its own schedule; kubs0 has maintenance windows.
Each host gets one cheap `ssh … exit` first; a host that does not answer is
recorded as `unreachable` and the run carries on to the next. The missed
sessions are picked up the next night — the accumulating mirror makes a
skipped run cost nothing but latency, and the ~30-day source window means even
a week of misses loses nothing.

What must *not* happen is a partial sync being mistaken for "nothing new". So
every run writes `sync-status.json`:

```json
{ "ran_at": "2026-08-25T11:00:02Z",
  "hosts": { "kai":   { "status": "ok", "transferred": 12, "secs": 3 },
             "kubs0": { "status": "ok", "transferred": 0,  "secs": 2 },
             "cleo":  { "status": "unreachable", "transferred": 0, "secs": 0,
                        "note": "did not answer ssh" } } }
```

`derive` copies it into `derived/` and the index page prints it, so "cleo —
not reached" is on the page a person looks at, in the place a count would be.

Three statuses. `ok` and `unreachable` exit 0; `failed` — a host that answered
and then failed mid-sync, a timeout, a missing tool — exits 1 so the systemd
unit shows `failed` and `just collect-status` says why. rsync's exit 24 ("some
files vanished during transfer") counts as `ok` with a note: that is exactly
what a self-pruning source does.

## The derive — `kagviz derive`

```
kagviz derive [--live DIR] [--out DIR] [--force] [--label …]
```

For every `<live>/<host>/projects/` (a host is any directory holding a
`projects/`; `derived/` has none and is never taken for one), every session:

1. Compute the **source digest**: sha256 over the transcript and each subagent
   sidecar — name, length and bytes, in `discover` order. Content, not mtime:
   a resumed session appends, a re-copy touches, and only the first should
   re-derive. (The proposal's open question, answered.)
2. Skip it if `state.json` records the same digest **and** the same kagviz
   version, and both outputs exist. Otherwise count it, write the facts (the
   same bytes `show --json` prints, trailing newline included, so a derived
   facts file diffs clean against a baseline) and the report, and record it.
3. Write `state.json` after each host, so a run that dies keeps what it did.

Then regenerate `sessions.json` and `index.html`, and write `META.json`.

**A kagviz upgrade regenerates all of `derived/`** without being asked: the
version in `state.json` is `<crate version> (<commit>)`, stamped by `build.rs`
at build time, and a session whose recorded kagviz differs from the running
one is re-derived. A changed extractor is changed facts. The full sweep over
the fleet is minutes. `--force` does the same on demand.

Every file is written to a temporary name and renamed into place, so
copyparty (below) never serves a half-written page.

One unreadable session is a warning and a count in the run — the other hosts'
work still lands — and the exit is non-zero so it is on the record.

`--label` runs the headline pass over each freshly counted session (see the
README): the cache lives in `derived/labels/`, never inside a mirror, and a
backend that is down degrades to "no headline", never to a failed derive. The
shipped timer does **not** pass it; add it to `ExecStart` when kvllm is
reliably up at 04:00, or run `just collect-derive --label` by hand.

`kagviz index [DIR]` regenerates only `sessions.json` and `index.html` from an
existing derived tree — a pure function of the facts files, `state.json` and
the sync status. Useful after a change to the page alone.

## Serving

Ken reads sessions from cleo and his phone, so the browse page wants HTTP.
copyparty on kai (`https://kai.encke-wahoo.ts.net:8027`, tailnet-only) serves
`derived/` read-only at `/kagviz/`. The page is
**<https://kai.encke-wahoo.ts.net:8027/kagviz/index.html>** — the bare
`/kagviz/` is copyparty's directory listing, not the table, because copyparty
shows a folder's listing rather than its `index.html` unless told otherwise.
Every report and facts link on the page is relative.

copyparty's `run.sh` and unit on kai are **rendered by k-homelab** from
`manifests/kai.yml` (`recipes/copyparty`, sprint 038 there) — the volume is
declared in that manifest and applied with `bin/apply kai copyparty`, not
edited on the host. The review had assumed a one-line `run.sh` edit; that file
carries a "GENERATED — do NOT edit on the host" banner, and the next apply
would have reverted it.

Said out loud, as the review asked: reports carry session content — prompt
previews, file paths, the user's own words in questions — and copyparty has no
accounts, so tailnet-only *is* the access control. That is the trust boundary
Ken already accepted for `~/src` on the same viewer (k-homelab manifest,
`copyparty.dotfiles.ack`); this is the same call, made visibly.

The same path carries the future front-end: a static app reading
`sessions.json` → facts → (later) events over HTTP needs no backend at all.

## Scheduling

A systemd **user** timer on kai, `OnCalendar=*-*-* 04:00:00`. kai's zone is
America/Los_Angeles, so that is 04:00 Pacific across DST. `Persistent=true`
runs a missed night at the next boot. The units are authored in `collect/`
and installed from there:

```sh
just collect-install   # cargo build --release, copy the units, enable --now the timer
just collect-status    # the timer, the last run, and sync-status.json
just collect-run       # run the unit once, right now, exactly as the timer would
```

The timer runs **this checkout's** `target/release/kagviz` (the kmon/kfdc
pattern). Which is to say: the branch checked out at 04:00 is the kagviz that
derives — a different commit re-derives everything, which is the rule, and it
takes minutes.

The unit sets `PATH` to include `~/.local/bin`, where rclone lives; a user
unit does not inherit a login shell's PATH and would otherwise report "rclone
is not installed" under the timer while `just collect` works (kfdc found the
same with `claude`).

## Operating

```sh
just collect                 # sync everything, then derive — what the timer runs
just collect-sync cleo       # one host, no derive
just collect-derive          # derive only, over what is already mirrored
just collect-derive --force  # re-derive everything
kagviz index                 # regenerate the index page alone
cat /ai-data/kagviz-data/live/sync-status.json
tail /ai-data/kagviz-data/live/sync.log
journalctl --user -u kagviz-collect.service -n 50
```

When `collect-status` shows `failed`: the unit's journal has the sync line
with the note (`rsync exit 23: …`, `rclone exit 1: …`, `timed out after
30m`). `unreachable` needs nothing — look at it only if the same host has been
unreachable for many nights, since the source window is ~30 days.

`sync.log` is append-only and unbounded: one line per host per night, ~80
bytes each, so a year is ~90 KB. Not worth rotating yet; noted so it is not
a surprise later.

## Decisions taken in sprint 007

- **rclone, not plain sftp, for cleo.** Incremental, never deletes, atomic
  per file. Installed user-scope on kai (`~/.local/bin/rclone`, the official
  zip — Ubuntu's package is 1.60, which predates the temp-then-rename write),
  and recorded as a machine change. Plain `scp -r` would have re-copied
  ~120 MB every night and rewritten every file in place.
- **Content hash, not mtime.** Above.
- **Subcommands, not a script.** `derive` and `index` are kagviz proper:
  they are the extractor and renderer applied to a tree, they carry the
  version stamp, and they are unit-tested. Only the host pulls — ssh, rsync,
  rclone, which hosts exist — are shell.
- **The label pass stays off in the timer** until kvllm is reliably up
  overnight. The mechanism is there; flipping it is one word in the unit.

## Not done here

- `kagviz sessions` still parses every transcript to print a table. The
  browse surface has moved to `derived/index.html`; the terminal command is
  unchanged and still slow on a large root.
- No events tier below the bucket; that is sprint 009's contract work.
- The index page is a static table, sorted newest first. Filtering and
  pan/zoom belong to the front-end, which reads the same `sessions.json`.
- `/kagviz/` still lands on copyparty's listing rather than the page.
  copyparty has `--ih` ("if a folder contains index.html, show that instead
  of the directory listing"), but it is a **global** flag — it would change
  every folder under `/src` that happens to hold an `index.html` too — and
  `run.sh` is rendered by k-homelab, so it is a recipe change to weigh there,
  not a tweak here. Until then, link `index.html`.
