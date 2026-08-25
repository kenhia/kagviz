# Review 006 — kagviz at MVP+

> First formal review. Sprint korg:1611 / WI #1610, 2026-08-24. The brief:
> the usual maintainability pass, but the primary question is **"are we moving
> in the right direction for this tool"** — toward an app where a session can
> be brought up, inspected, and dug into.

## Where the project stands

Five sprints past the scaffold, the deterministic core is real and proven:

- **The extractor** parses 405 real transcripts across three hosts and two
  platforms with zero parse failures and zero skipped lines, over 32 CLI
  versions of an undocumented, drifting format.
- **The facts document** is a working contract: five additive changes and one
  breaking change so far, the breaking one landed with before/after measured
  over the whole corpus and the unmoved fields named precisely
  (`docs/facts-contract.md`).
- **The report** renders the facts self-contained, byte-identical across
  platforms and across the serialize/deserialize seam, with idle collapsed,
  phases banded, and the user's involvement marked.
- **The model seam** held on first contact: `labels` is the only written
  field, opt-in, cached on a facts digest, and proven additive (405/405
  byte-identical without the flag).
- **The pinned corpus + baselines** under `/ai-data/kagviz-data` are the
  reason all of the above is checked rather than believed. The corpus caught
  a real defect on its first day, and sprint 005's breaking change shipped
  with exact impact numbers because the baseline existed.

Weight: ~6.2k lines of Rust in seven modules; 76 tests; `just check` runs
fmt + clippy `-D warnings --all-targets` + tests.

## Direction verdict: right direction, and the early bets are paying

The destination Ken named is an **app**: bring up a session, inspect it, dig
in — click a timeline segment and see its records and tool calls, which
failed, what was said. Measured against that destination:

**Two early decisions are exactly what an app needs, and both were made
before the app existed.**

1. **Facts-as-contract.** The interactive front-end was always the stated
   consumer of `show --json` — that is *why* it is a contract. Nothing about
   the static report needs unwinding; the front-end plugs into the same seam.
2. **Determinism + pinned baselines.** An app grows by changing the facts.
   The corpus/baseline discipline is what makes facts changes cheap to make
   and safe to trust — sprint 005 demonstrated the full loop.

**Four things separate today's tool from that app**, and they name the next
phase of work:

1. **Sessions don't survive.** The live transcript store self-prunes on a
   ~30-day window — a session vanished *mid-sweep* during sprint 003. Without
   collection there is eventually nothing to browse. This is why collection
   scaffolding is the priority, not a convenience.
2. **No browse surface.** `kagviz sessions` lists one root on one host.
   "Which session do I want to dig into" needs a cross-host index with enough
   on it to choose by.
3. **No detail below the bucket.** The facts carry per-bucket *counts* —
   the hover tooltip's "15 records, 4 tool calls, 1 failed" is the floor of
   what today's document can say. Click-for-detail needs facts that carry the
   events themselves. This is the one place the contract must *grow* before
   the app can exist.
4. **No interaction layer.** The report is deliberately static, and
   `render::tests::the_report_is_self_contained` enforces it. Interaction
   beyond CSS tricks means a front-end that is its own artifact — the
   long-signposted TS app — not a relaxation of the static report's rules.

One bet to *stop* doubling down on: the static renderer. It is 1.6k lines of
hand-built HTML and it has done its job — proving the facts are worth looking
at. Report work from here should be limited to cheap legibility wins (#1590,
the zoom checkbox on #1591); pan/zoom/drill-down ambitions belong to the
front-end, where the facts contract already points.

## Thread 1 — collection scaffolding (the priority)

### Shape

Keep `/ai-data/kagviz-data` as the home. The pinned `corpus/` and
`baselines/` stay exactly what they are — immutable snapshots with a date and
a commit on them. Collection adds a **live, accumulating mirror** beside
them:

```
/ai-data/kagviz-data/
    corpus/<host>-<date>/      # pinned, immutable (unchanged)
    baselines/<host>-<date>/   # pinned, immutable (unchanged)
    live/
        kai/projects/          # verbatim mirror of ~/.claude/projects
        kubs0/projects/
        cleo/projects/
        derived/               # everything computed from the mirrors
            facts/<host>/<session>.json
            reports/<host>/<session>.html
            sessions.json      # the cross-host index, machine-readable
            index.html         # the browse page, human-readable
            META.json          # kagviz commit + when, so derived is auditable
```

Rules the pinned store already taught us, applied to `live/`:

- **Mirror, never prune.** The source deletes after ~30 days; the mirror is
  where history survives. Sync copies new and updated files and never
  propagates a deletion.
- **Verbatim raw, everything else derived.** `live/<host>/projects` is
  transcript bytes exactly as written (sidecars and `tool-results/`
  included). Anything computed goes under `derived/`, stamped with the
  kagviz commit that produced it, and is regenerable at will — a kagviz
  upgrade regenerates all of `derived/` (the 405-transcript sweep is already
  a routine that takes minutes).

### Mechanism

Runs on kai, which owns `/ai-data` (local NVMe, 3.1T free):

- **kai** — local rsync.
- **kubs0** — rsync over ssh (both ends have rsync).
- **cleo** — Windows, no rsync; sshd is up and reachable from kai (verified
  this session). Pull over sftp — rclone on kai is the likely tool (not
  currently installed; installing it is a recorded machine change), with
  plain `scp`/`sftp` as the fallback. cleo's transcripts live under
  `C:/Users/kenhi/.claude/projects`.

**An unreachable host is a normal night, not a failure.** cleo sleeps
occasionally and Windows Update reboots it on its own schedule; kubs0 could
be down for maintenance. The collector treats each host independently: a
host that does not answer is skipped with a note in the run log, the other
hosts still sync, and the derive stage still runs over whatever arrived.
The missed sessions are simply picked up the next night — the accumulating
mirror makes a skipped run cost nothing but latency, and the ~30-day source
window means even a week of misses loses nothing. What must *not* happen:
one dead host aborting the run, or a partial sync being mistaken for "host
had nothing new" — the run log says which hosts were reached, so an absence
is visible rather than silent (the same rule the facts already live by).

After sync, the derive stage runs kagviz over new/updated sessions: facts,
report, then the regenerated index. If kvllm is up, `--label` can join the
nightly derive — the cache keys on the facts digest, so labels stay
reproducible and the backend being down degrades to "no headline", never a
failure.

### Scheduling

A systemd **user** timer on kai, `OnCalendar=*-*-* 04:00` — kai's zone is
America/Los_Angeles, so that is the 0400 PT Ken asked for. Per homelab
convention the unit files are authored in this repo and installed from it
(`just collect-install` or similar); the same entry point runs manually as
`just collect`. Installing the timer gets a `record-machine-change` entry.

### Serving

Ken reads sessions from cleo and his phone, so the browse page wants HTTP,
not a kai-local path. The stop-gap copyparty on kai
(`https://kai.encke-wahoo.ts.net:8027`, tailnet-only) exists for exactly
this kind of self-contained HTML; adding a read-only volume for
`/ai-data/kagviz-data/live/derived` is a one-line change to its `run.sh`.
Worth saying out loud: reports carry session content (prompt previews, file
paths), and copyparty has no accounts — tailnet-only is the access control.
That is Ken's stated trust boundary for `~/src` already; this is the same
call, made visibly.

The same serving path carries the future front-end: a static SPA reading
`sessions.json` and per-session facts over HTTP needs **no backend at all**,
which keeps the facts contract as the only seam.

### Open questions for the implementing sprint

- rclone vs plain sftp for the cleo pull (incrementality vs one less
  install).
- Whether the derive stage detects "updated session" by mtime/size or by
  content hash — resumed sessions append, so either works; hash is the one
  that cannot be fooled.
- Whether `sessions.json` + `index.html` generation is a new `kagviz index`
  subcommand (it is a pure function of a set of facts documents, so it
  belongs in kagviz proper) or a script beside the collector. Leaning
  subcommand.

## Thread 2 — interactivity

Three horizons, each with its own vehicle:

**Now (static report, CSS only).** Ken's #1591 comment proposes a zoom-in
checkbox for dense strips. A checkbox + `:checked` sibling selector can
render the strip at readable element size inside a horizontal scroller with
**zero JavaScript** — the self-contained property survives intact. Bundle
with #1590 (failure rate beside the count): two small legibility wins in one
report-touching PR.

**Next (the facts grow a detail tier).** The contract question at the heart
of the app: today's floor is bucket counts; click-for-detail needs per-event
data — tool name, timestamps, ok/failed, joined result sizes, per-turn
structure. Constraints already known:

- Additive, obviously — but *where* is a real design decision: inline in the
  facts document (one document, but a 12-hour session's facts grow from
  ~100KB toward megabytes) vs a **separate events document** the same
  contract discipline applies to (`show --events`?), letting the summary
  stay light and the front-end lazy-load detail. Leaning separate document —
  "forest, tree, leaf" wants the leaf fetched on demand.
- `MAX_BUCKETS = 240` caps what any timeline can resolve (30-minute columns
  on a 12-hour session). The zoom the app wants needs either finer buckets at
  deeper zoom levels or buckets derived client-side from events. This is the
  ceiling sprint 002 deliberately did not revisit; the events tier is where
  it gets revisited.

**Then (the front-end).** A TS SPA in this repo (`web/`), served static off
the same copyparty/successor, reading `sessions.json` → facts → events.
Timeline pan/zoom (#1591 proper), click a segment → the drill-down Ken
described. No backend; the contract is the API. This is deliberately *after*
collection and the events tier: the app is only as good as the data under
it, and both of its inputs come from those two sprints.

**No program filed.** Everything above lives in this one repo — korg's
program layer is for cross-project work, and single-project sequencing is
what the proposal queue's ranks are for. If `web/` ever becomes its own
repo, that is the moment a program earns its place.

## Maintainability

Code-health pass over all of `src/` (76 tests green). **Verdict: the stated
boundaries hold.** The renderer imports no transcript code outside
`#[cfg(test)]` and `render --from facts.json` proves the seam; label failure
is a warning, never an error, and the no-quantities brief is enforced by
test; `summarize` takes pre-read bytes and contains no clock, env, or float.
transcript.rs is a model tolerant reader; label.rs's cache/digest design is
genuinely careful; the convention spot-checks (null_as_default coverage,
opaque-never-folded, is_user_turn discrimination, annotated dead code) all
pass.

Five risks worth planning around, ranked:

1. **The contract says "absent", the serializer says `null`.** Verified on a
   live `show --json`: `opened_by`, `chosen`, and the unjoined-spawn fields
   emit `"field": null` where `docs/facts-contract.md` says *absent* —
   `labels` is the only field with `skip_serializing_if`. Given this
   project's own null-vs-absent trap, this is the sharpest drift found, and
   it is a byte-level breaking change that gets more expensive with every
   consumer. **Fix it now, while there are zero external consumers** — and
   before the front-end exists. (→ WI, sprint 009)
2. **Twin counting passes.** `summarize` and `summarize_spawn` duplicate the
   token/tool/failure accumulation; the next quantity added to one tier and
   not the other silently makes the tiers non-comparable. Extract one shared
   per-record accumulator before the events tier lands. (→ WI, sprint 009)
3. **A facts addition fans out across 4–6 sites**, and the easy one to miss
   is `show_session` in main.rs — a third presentation layer beside
   render.rs and the contract doc. A short "adding a facts field" checklist
   in CLAUDE.md names them all. (→ done in this sprint)
4. **The real guarantees are unreproducible from a clone, and no CI runs the
   gate.** The 405/405 sweeps live on `/ai-data`; `.github/` has no
   workflow. The roadmap's hand-minimised in-repo fixture plus a CI job
   running `just check`, plus one golden-file render test to replace the
   copy-pinned substring probes, close this together. (→ WI, sprint 009)
5. **render.rs is hand-built HTML** — fine for the static report, brittle
   if interactivity is ever grown there. The mitigation is the direction
   verdict above: legibility tweaks only; interaction goes to the front-end.

Smaller hygiene, filed with the sprint-009 WIs rather than fixed here (this
sprint ships a review, not code): render.rs's "nothing here computes a
number" doc comment overstates (it computes presentation sums), a stale
`#[allow]` on `Content::Text`'s field which non-test code now reads,
`subagents` deduped inconsistently with `skills`, and stale corpus counts
(305/197) in the contract doc and roadmap. Also noted for sprint 007's
design: `kagviz sessions` fully parses every transcript just to print a
table — the derive stage's precomputed facts are what fixes that cliff.

## The plan

Filed in korg, in queue order:

1. **Sprint 007 — collect the fleet, nightly** (proposal + WIs): the
   scaffolding above. Priority per Ken.
2. **Sprint 008 — the report answers a click** (proposal + WIs): the events
   tier design + implementation, the zoom checkbox, #1590.
3. **Front-end v1** (WI, no proposal yet): queued want; propose when 007 and
   008 have landed and the shape of the events document is proven.

Roadmap updated to match (including the missing sprint-005 Shipped entry —
found by this review).
