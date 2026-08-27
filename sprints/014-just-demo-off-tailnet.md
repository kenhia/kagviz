# Sprint 014 — `just demo`: kagviz, shown to people who are not on the tailnet

korg:1649 · #1648 · branch `014-just-demo-off-tailnet`

## Goal

One recipe that puts kagviz on a URL a machine outside the tailnet can open.

The proposal shipped with two halves: the transport (there is no LAN path to
kagviz at all) and a curation/audit step (the live tree is 413 sessions of
prompt previews across every project). Ken cut the second at the start:

> from a demo perspective — at least at this point we don't need to "clean" for
> demo; I would select what I would show and pre-check it. So this sprint
> should be primarily about temporarily serving where `kwork` (non-tailnet) can
> reach, not checking what we would serve.

So the sprint is transport plus a selection knob. The recipe prints what is in
the tree; the person running the demo decides whether that is showable.

## Why there is no LAN path, which is the whole reason for the sprint

copyparty binds `127.0.0.1:8027`. The `:8027` listeners on kai's *tailnet* IP
belong to `tailscaled` — `tailscale serve` terminating TLS and proxying to
loopback. Nothing on the LAN is listening. `kwork` is Microsoft-managed and
deliberately off the tailnet (homelab TLS/auth decision, 2026-07-11) but
reaches the LAN fine, so an ATV-group demo over Teams needs a second,
temporary server. `ufw` is inactive on kai, so nothing else blocks a port.

Both halves of that were found by building the thing, not by reasoning about
it — which is also why the proposal insisted this was a sprint and not three
lines in a terminal.

## What shipped

`collect/demo.sh` and two recipes:

```sh
just demo                     # kagviz's own sessions, built and served
just demo '*korg*' '*kmon*'   # quote the globs — your shell would eat them
just demo --build-only        # build the tree, do not serve (pre-check)
just demo --serve-only        # serve what is already built, no rebuild
just demo --port 9000         # default 8028
just demo-clean               # remove the tree
```

It copies matching project directories out of the live mirror into
`~/.cache/kagviz-demo/<host>/projects/`, derives, installs the app, re-indexes,
and serves `derived/` on this host's LAN address with `python3 -m http.server`.

The default corpus is kagviz's own sessions: a public repo, no other project's
paths in the tree, and the tool showing the sessions that built it.

## The five things the three obvious commands do not do

The proposal's proven recipe was `derive` → `web-deploy` → `http.server`.
Four of these five are the difference between that and something you can run
while someone is watching; one of them is a bug in the three lines.

- **The LAN address comes from the default route**, not a hardcoded `enp10s0`.
  A wrong address fails silently as "connection refused" mid-meeting.
  Tailscale's routes are per-host `/32`s and never the default, so this cannot
  pick the tailnet IP by accident; `KAGVIZ_DEMO_ADDR` overrides, and an
  address inside `100.64.0.0/10` is called out as suspicious rather than
  silently served.
- **`kagviz index` runs again after `web-deploy`.** This is the bug in the
  three lines. `derive` regenerates `index.html` last and links the app *only
  when `derived/app/index.html` is already on disk* — deliberately, so a
  reader can tell "not deployed" from "broken". Run in the proposal's order,
  the demo's browse page ships with no way into the app, which is the half a
  demo is actually for. `docs/collection.md` already recorded this trap for
  the production deploy; the demo walked into the same one from the other
  direction.
- **The URL is printed with `index.html` on it.** `python3 -m http.server`
  does serve a directory index, unlike copyparty — verified, the bare URL
  returns the table — but the two hosts should not have to be remembered
  differently.
- **Plaintext HTTP on the LAN is said out loud, once.** Consistent with the
  accepted kwork↔korg posture, but a stated choice rather than an unnoticed
  one.
- **The tree is rebuilt from scratch every run**, so last week's corpus cannot
  turn up on a projector.

## Decisions

**It selects; it does not audit.** Per Ken's call above. The recipe prints the
project directories copied, the session count per host, and the served size,
then says in as many words that the prompt previews are the user's own words
and nothing here checked them. No secret scan, and nothing downstream should
treat this as a safety gate. Recorded on korg:1649 so sprint 015 plans against
it rather than against the proposal's original assumption — see Follow-ups.

**`sync-status.json` is not copied.** The work item left this open and asked
for a deliberate pick. It reports the collector's last run over the whole
fleet, which says nothing true about a hand-picked tree, and "cleo — not
reached" is infrastructure noise in front of an audience. Both consumers
already render the absence honestly *by design*: the static page prints "no
sync status recorded — the collector has not run, or these mirrors were not
written by it", which is precisely the case, and the app's `loadSyncStatus`
catches the 404 into `undefined` for `SyncLine` to say "no sync status". The
one visible cost is a `sync-status.json` 404 in the browser console — the
designed-for path, not a defect.

**The tree survives the server.** Ctrl-C stops the server and the trap says
so; the tree stays at `~/.cache/kagviz-demo` so `--serve-only` can put it back
in seconds mid-meeting, and `just demo-clean` removes it. "Tear down cleanly"
is about the listener, which does not outlive the terminal.

**The script lives in `collect/`.** It reads the live mirror and drives
`derive`, which is what everything else in that directory does, and
`docs/collection.md` already owns the "Serving" story it extends.

**The build output is captured and shown only on failure.** The npm build is a
screenful, and the one line `web-deploy` prints names copyparty's mount rather
than this one — a wrong instruction in the middle of demo prep.

## Verification

Everything below was run for real on kai; there is no unit test here, because
there is nothing in this sprint that a unit test would have caught. The
extractor, the facts, the contract and the app are untouched.

- `just demo --build-only` — 10 sessions, 2.0 MB served (32 MB on disk with
  the copied mirror), whole run 1.4s.
- `just demo '*kagviz*' '*korg*' --build-only` — 60 sessions across two
  project directories, 12 MB served. Multi-glob selection works.
- `just demo 'no-such-project-*'` — exits 2, lists every available
  `host:project` so the next attempt can be right, and removes the tree it had
  started rather than leaving a half-state for `--serve-only` to trip over.
- `just demo --serve-only` — bound `192.168.1.109:8028`, the LAN address, not
  loopback and not the tailnet IP.
- Driven headless from that origin (`.scratch/014/drive-demo.mjs`, playwright):
  static browse page 10 rows with an `app/index.html` link present, the bare
  directory URL 200 with the same 10 rows, the app 10 rows, a session page
  reached through the app's own link with its events loaded and the timeline
  captioned, and a static report 200 on the same mount. One console entry: the
  `sync-status.json` 404 above.
- `just demo-clean` removes the tree.

## Follow-ups

- **Sprint 015 (korg:1659) must plan against this, not against the proposal.**
  015 would serve each tool call's input and result text — the first raw
  session content on a served surface — and was ranked behind this sprint on
  the assumption that whatever curation `just demo` built was the thing it
  would reuse. That assumption is now false: curation here is a human
  pre-check, so there is no mechanism to reuse and nothing to treat as a gate.
  015 should ship `calls/` **off by default** (`derive` writes it only when
  asked; the demo tree never contains it), which keeps this sprint's exposure
  surface provably unchanged. Recorded as a comment on korg:1649.
- The audit half of the original work item is not lost, just not this sprint's
  job. If it ever comes back it should be its own item, because the measured
  numbers behind it (~51 plausibly-live credentials across 59 of 413 sessions,
  90,182 payloads) are about *call text*, which is 015's surface, not the
  browse page's.

## Deployed

**2026-08-26**, kai, from merged `main` at `d8d99b2` (PR #15, squash-merged
after CI passed on the branch and again on the merge commit).

| what | where | result |
|---|---|---|
| extractor | `target/release/kagviz` in this checkout | rebuilt at `d8d99b2` — the binary the 04:00 timer runs |
| app bundle | `/ai-data/kagviz-data/live/derived/app/` | deployed **before** the derive, so the browse page carries its link |
| served tree | `…/derived/` | re-derived, 413 sessions across cleo/kai/kubs0 in 1.3s |

- **Stamp**: `META.json` reads `0.1.0 (d8d99b2)`, matching `HEAD`.
- **Bytes**: `sha256sum` over `facts/`, `events/` and `reports/` before and
  after — **1,239 documents, zero moved**. That is the proof this sprint
  wanted: it is packaging, it did not touch the extractor, and nothing about
  the facts changed. The stamp moved; no value did.
- **Served**: `/kagviz/index.html` 200, `/kagviz/app/index.html` 200, and the
  browse page carries one `href="app/index.html"` — the step-3 ordering trap,
  caught rather than assumed.
- **The sprint's own deliverable**, smoke-tested from the merged checkout
  rather than the branch: `just demo --build-only` → 10 sessions / 2.0 MB
  served; `just demo --serve-only` bound `192.168.1.109:8028`; driven headless
  from that origin — static page 10 rows with the app link, bare directory URL
  200, app 10 rows, a session page reached through the app's own link with its
  events loaded, a static report 200. One console entry, the expected
  `sync-status.json` 404. `just demo-clean` removed the tree and the port is
  clear.

Rollback target: `b838fcc` (the commit before this sprint). `derived/` is
disposable — the mirrors were not touched, as always.
