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
        events/<host>/<session-id>.json    # `kagviz show --events` — the detail tier
        calls/<host>/<session-id>.json     # `kagviz show --calls` — ONLY with --calls
        reports/<host>/<session-id>.html   # `kagviz render`
        sessions.json        # the cross-host index — a contract (facts-contract.md)
        index.html           # the page a person picks a session from
        app/                 # the front-end (sprints 011-012) — `just web-deploy` puts it here
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
  will — delete the whole directory and the next `derive` rebuilds it. `app/`
  is the one thing under `derived/` a *run* does not rebuild: it is produced
  by the build (`just web-deploy`) rather than by `derive`, and `derive` and
  `index` never write into it. It is still regenerable, from the same
  checkout, which is what the rule is for.

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
kagviz derive [--live DIR] [--out DIR] [--force] [--calls|--drop-calls] [--label …]
```

For every `<live>/<host>/projects/` (a host is any directory holding a
`projects/`; `derived/` has none and is never taken for one), every session:

1. Compute the **source digest**: sha256 over the transcript and each subagent
   sidecar — name, length and bytes, in `discover` order. Content, not mtime:
   a resumed session appends, a re-copy touches, and only the first should
   re-derive. (The proposal's open question, answered.)
2. Skip it if `state.json` records the same digest **and** the same kagviz
   version, and every output exists. Otherwise count it, write the facts (the
   same bytes `show --json` prints, trailing newline included, so a derived
   facts file diffs clean against a baseline), the events document (likewise,
   against `show --events`) and the report, and record it. With `--calls`,
   the calls document too — see below.
3. Write `state.json` after each host, so a run that dies keeps what it did.

Then regenerate `sessions.json` and `index.html`, and write `META.json`.

### `--calls`, and why it is off

`derive` writes `calls/` **only when asked**. Everything else under `derived/`
is counted *from* the transcripts; the calls document is the transcripts' own
payload text — command output, file contents, pasted material, and, on 59 of
413 live sessions, something credential-shaped. The mirrors it comes from are
not served at all, so this is the one thing in the tree whose presence is a
decision rather than a consequence, and **the flag is that decision**.

Three consequences worth knowing before you run it:

- **The nightly timer never writes it.** `collect/kagviz-collect.service` runs
  a plain `derive`, so the served tree stays in the default state unless
  someone opts in by hand.
- **Opting in re-derives.** `state.json` records the transcript bytes and the
  kagviz version, neither of which changes when you add `--calls`, so the run
  would otherwise report every session unchanged and write nothing. `derive`
  checks that every output *it was asked for* is present, which is what makes
  the first opt-in run actually do something.
- **A later plain run leaves it alone**, because a run that was not asked
  about call text does not get to decide about it. `--drop-calls` is the way
  back to the default state; `index` reads the tree rather than the flag, so
  `sessions.json` links `calls` exactly where a file exists.

It costs about 114 MB over the 405-session corpus (median 204 KB a session,
max 4.5 MB) on a volume already holding ~594 MB of mirrors. See
`docs/facts-contract.md` and `sprints/015-the-leaf-opens.md`.

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

The same path carries the front-end, and since sprint 011 it does:
**<https://kai.encke-wahoo.ts.net:8027/kagviz/app/index.html>** is the app,
reading `sessions.json` → facts → events over HTTP with no backend at all.

Since sprint 012 a session page also fetches that session's **events**
document, which is the largest thing this mount serves: tens of KB typically,
**2.6 MB** on the corpus's worst session. It is fetched alongside the facts
rather than before them — the page renders fully without it, and the timeline
draws at the facts' own resolution until it lands — and the app shows the bytes
as they arrive rather than a spinner. Nothing else changes about the mount: it
is one more static file `derive` already wrote. `just web-deploy` builds `web/` and stages it into
`derived/app/`; the static `index.html` links it, but only once it is actually
there — a link to a 404 would leave the reader unable to tell "not deployed"
from "broken".

Two properties of this mount are load-bearing, and both were learned by
deploying rather than by reasoning:

- **`/kagviz/app/` is a directory, and copyparty serves a directory as a
  listing** — the same trap recorded below for `/kagviz/`. So the entry point
  is `app/index.html`, and every in-app link is a bare `#/…` fragment resolved
  against the document. `resolve()` from SvelteKit's `$app/paths` produces
  `/kagviz/app#/…` — the directory — and following one of those leaves the app
  for a file listing.
- **The app is mounted, not rooted.** Its asset URLs are rewritten relative at
  build time (`web/scripts/relativize.js`), so the bundle works wherever the
  directory is copied and the mount path is not baked into it.

## Showing it to people — `just demo` (sprint 014)

That mount is **tailnet-only, and there is no LAN path at all**. copyparty
binds `127.0.0.1:8027`; the `:8027` listeners on kai's tailnet IP belong to
`tailscaled` (`tailscale serve` terminating TLS and proxying to loopback). So
a machine that is not on the tailnet cannot reach kagviz by any URL — and
`kwork` is Microsoft-managed and deliberately off the tailnet (homelab
TLS/auth decision, 2026-07-11) while reaching the LAN fine. Showing kagviz
over Teams needs a second, temporary server.

```sh
just demo                     # kagviz's own sessions, built and served
just demo '*korg*' '*kmon*'   # quote the globs — your shell would eat them
just demo --calls             # include the tool calls' own text (015)
just demo --build-only        # build the tree, do not serve (pre-check)
just demo --serve-only        # serve what is already built, no rebuild
just demo --port 9000         # default 8028
just demo-clean               # remove the tree
```

`collect/demo.sh` copies the matching project directories out of the live
mirror into `~/.cache/kagviz-demo/<host>/projects/`, derives, installs the app
and serves `derived/` on this host's LAN address with `python3 -m
http.server`. A curated corpus is nearly free because `derive` runs over any
directory holding `<host>/projects/`.

Five things it does that the three obvious commands do not:

- **Resolves the LAN address from the default route** rather than hardcoding
  an interface. The address is not a constant and a wrong one fails silently
  as "connection refused" mid-meeting. Tailscale's routes are per-host `/32`s
  and never the default, so this cannot pick the tailnet IP by accident;
  `KAGVIZ_DEMO_ADDR` overrides, and an address inside `100.64.0.0/10` is
  called out as suspicious.
- **Re-runs `kagviz index` after `web-deploy`.** `derive` writes `index.html`
  before `derived/app/` exists, so the "Open the app" link the page normally
  carries is absent from the page `derive` just wrote — the link is only
  emitted when the app is actually there. Without the second `index` pass the
  demo's browse page has no way into the app, which is the half a demo is for.
- **Prints the full URL with `index.html` on it.** `python3 -m http.server`
  does serve a directory index, unlike copyparty, so the bare URL works too —
  but the two hosts should not have to be remembered differently.
- **Says once that this is plaintext HTTP on the LAN**, no TLS and no
  accounts. Consistent with the accepted kwork↔korg posture, but a stated
  choice rather than an unnoticed one.
- **Rebuilds the tree from scratch every time**, so last week's corpus cannot
  turn up on a projector.

**It selects; it does not clear.** Ken's call at the start of 014: the person
running the demo picks what to show and pre-checks it, so the recipe's job is
the transport and the selection knob. It prints what is in the tree — the
projects copied, the session count per host, the served size — and says in as
many words that the prompt previews are the user's own words and nothing here
checked them. **Nothing downstream may treat this as a safety gate.**

Sprint 015 added `--calls` and, with it, an **exposure floor** to the
pre-check — a reporter, not a redactor, and the distinction is the design.
It greps the *served* tree (`derived/`, the bytes the room can actually
fetch, never the copied mirror beside it) for five known credential shapes
and prints what it found. It scales with the choice that was made, which is
the point of scanning what will be served rather than what was selected:

```
             default          --calls
scanned      70 files         92 files
matched      0 of 5 shapes    81:  private-key 5  sk-ant 15  KEY=value 58  dsn-password 3
```

That is one real project's 22 sessions, both ways — and it is the whole
argument for `--calls` being off by default, in one screen, where the person
about to share it is standing.

What it can never say is "clean". It matches shapes someone thought of, so
its zero is a fact about the scanner and not about the tree, and it prints
that in as many words. **A redactor's clean pass is a claim about the text; a
floor-reporter's zero is a claim about the scanner** — only the second can be
true, which is why 015 built the second and rejected the first. Adding a
pattern is welcome and changes none of that reading.

**`sync-status.json` is deliberately not copied.** It reports the collector's
last run over the whole fleet, which says nothing true about a hand-picked
tree. Both consumers already render the absence honestly and by design — the
static page prints "no sync status recorded — the collector has not run, or
these mirrors were not written by it", which is exactly the case, and the
app's `loadSyncStatus` catches the 404 into `undefined` and `SyncLine` says
"no sync status". The one visible cost is a 404 for `sync-status.json` in the
browser console, which is the designed-for path and not a defect.

Nothing here changes the facts or the contract. It is packaging, and it uses
the property `web/README.md` records — the app works under any HTTP mount at
any depth — without spending it. `--calls` is the one thing it can put on the
wire that a default `derive` would not, and it is off unless typed.

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

## Deploying

**The deploy artifact is `target/release/kagviz` in this checkout.** The unit
sets `WorkingDirectory` here and `collect.sh` runs that path directly — it does
not build. So whatever binary is in `target/release/` at 04:00 is the extractor
that derives the fleet, and a ship that does not rebuild leaves the collector on
the previous sprint's code.

`just collect`, `just collect-derive` and `just collect-install` all depend on
`build-release`; the **systemd unit does not**. That asymmetry is the trap.

Sprint 011 found the served tree stamped `0.1.0 (19a75d4)` — a sprint-009
*branch* commit that squash-merge had collapsed, so it named no commit
reachable from `main`. Three sprints, because nothing in the workflow rebuilt
after a ship. `.sprint-deploy` now declares `deploy-kagviz`
(`.claude/skills/deploy-kagviz/SKILL.md`), which sprint-ship runs in Phase 7,
after the merge — so what derives the fleet is built from merged `main`.

Two things that skill encodes and are easy to get wrong by hand:

- **`just web-deploy` before `just collect-derive`.** `derive` regenerates
  `index.html` last, and the browse page links the app only when
  `app/index.html` is already on disk. Reverse them and the page ships without
  its link.
- **A sprint that did not change the extractor must move zero derived bytes.**
  The stamp changes; no value does. Sweep `sha256sum` over `facts/`, `events/`
  and `reports/` before and after — 1,221 documents today — and diff it. That
  diff is the deploy's proof, and it is also how an unintended change to the
  facts gets caught at the moment it ships rather than months later.

Rollback is cheap because nothing here is a source of truth: the mirrors are
never touched and never pruned, so `derived/` rebuilds from them in seconds.

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
- The index page is a static table, sorted newest first. Filtering and
  pan/zoom belong to the front-end, which reads the same `sessions.json` —
  and, since 009, the events document each row links, for what a click on a
  segment shows.
- `/kagviz/` still lands on copyparty's listing rather than the page.
  copyparty has `--ih` ("if a folder contains index.html, show that instead
  of the directory listing"), but it is a **global** flag — it would change
  every folder under `/src` that happens to hold an `index.html` too — and
  `run.sh` is rendered by k-homelab, so it is a recipe change to weigh there,
  not a tweak here. Until then, link `index.html`.
