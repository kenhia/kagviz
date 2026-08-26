# Sprint 009 — the facts learn detail, and the contract gets honest

korg:1622 · covers #1617 (one accumulator for both counting tiers),
#1616 (emit absent, not `null`), #1615 (the events detail tier),
#1618 (in-repo fixture, golden render test, CI)

## Goal

The contract work the app needs, bundled so one baseline regeneration covers
all of it — sprint-005 economics. Four items in a deliberate order: the pure
refactor first, while the corpus can prove it moved nothing; then the
breaking change, measured; then the additive events document on top of the
now-shared accumulator, so the new tier is counted by the same code in both
places; and the fixture + golden test + CI, which make the guarantees
reproducible from a clone rather than from `/ai-data`.

## The anchor, checked rather than assumed

Sprints 006, 007 and 008 claimed to move no facts. Before touching anything,
`main` (e6355b9) was built and swept over the pinned corpus: **405 of 405
facts files byte-identical to the sprint-005 baseline `a8dad05`**, 0 skipped
lines. So every "0 differ" below is against a baseline the current code
actually reproduces, not one it is assumed to.

A lesson from the first attempt at that proof: a chain of the form
`cargo build | tail -1 && sweep` swallows the build's exit status, and the
sweep then ran the *previous* binary and matched the baseline perfectly. The
sweep scripts in `.scratch/009/` now build unpiped and chain on `&&`.

## What shipped

### #1617 — one accumulator for both counting tiers

`summarize` and `summarize_spawn` carried two copies of the per-record walk:
turns, tokens, tool calls, failures blamed through the call table, the
file-change tally, timestamps. Both now count through one `Counter`
(`count(rec) -> Counted`, `finish() -> Counts`). What a session has that a
spawn does not — the tool mix a phase is named from, questions, skills, the
spawn joins, user turns — is layered *beside* the shared count in
`summarize`, never inside it.

Proof: 87 tests green, and the release binary reproduces `a8dad05` on
**405 of 405** sessions. Pure refactor, as the proposal required.

Hygiene in the same commit: render.rs's module doc no longer claims "nothing
here computes a number" (it computes presentation sums, which the contract
allows — it now says "no *fact*" and names them); the stale `#[allow]` on
`Content::Text` is gone, since `is_user_turn` and `preview_of` read it; the
contract's "measured over 305 transcripts" reads 405. The roadmap's own
305/197 are in its *Shipped* history and were left as the record they are.

The `subagents` dedup the WI also asked for changes facts bytes, so it moved
to #1616's measured batch rather than into this byte-identical commit.

### #1616 — emit absent, not `null`

Review 006's sharpest finding, verified: the contract said `opened_by`,
`chosen` and the unjoined-spawn fields were *absent*, and the serializer
wrote `"field": null` for every one — `labels` was the only field carrying
`skip_serializing_if`. Every `Option` the document carries now does:
`opened_by`; a question's `at`/`header`/`chosen`; a prompt's `at`; a spawn's
`agent_id`/`subagent_type`/`description`/`model`/`started`/`ended`; the
top-level `session_id`/`project`/`cwd`/`git_branch`/`started`/`ended`.

Measured over the corpus, against `a8dad05`:

| | |
|---|---|
| sessions whose bytes change | **397 of 405** |
| values that move | **0** |
| what had been `null` | `opened_by` 1,971× (resumed phases) · `chosen` 6 · `subagent_type` and `description` 5 each (sidecars no `Agent` result joined) · `cwd`, `git_branch`, `started`, `ended` once each (one session with no timestamped record) |

The proof is the normalisation: strip the `null`s from the old baseline and
every one of the 405 documents is byte-identical to the new output — except
the eight below.

**`subagents` is now the set.** Two `Explore` spawns rendered two identical
chips; `skills` had been deduped since 001. 8 of 405 sessions: 21 entries →
9 (`Explore,Explore` → `Explore`; one cleo session had five
`general-purpose`). Dedup those in the normalised baseline as well and the
count reaches **0 differ on all three hosts**. Both changes are rows in the
contract's breaking-changes table; the contract's rules gained the
absent-never-`null` bullet the sessions.json section had already been
promising on the facts' behalf.

Consumers: none external yet, which is the whole reason to do it now. The
label cache invalidates, correctly — the digest is over the serialized facts.

### #1615 — the events detail tier

The contract question at the heart of the app, answered by building it: the
facts carry per-bucket *counts*, and a click on the timeline needs the
things that were counted. `kagviz show <id> --events` now emits them, and
`derive` writes them beside the facts as `derived/events/<host>/<id>.json`,
linked from every `sessions.json` row.

**Where: a separate document.** Review 006 leaned that way and the numbers
confirm it — the whole tier over the 405-session corpus is 39 MB against
the facts' few, and the 12h39m, 209-span session alone is **2.6 MB** of
events (the next largest, 621 KB). Inlined, that would sit inside a facts
document every consumer fetches to draw a row. "Forest, tree, leaf" wants
the leaf fetched on demand, so it is its own file under the same contract
rules, with its own section in `docs/facts-contract.md`.

**What: a flat, tagged list.** Mirroring `user_involvement`'s shape rather
than nesting — one time-ordered list a consumer filters, tagged by `kind`:

- `turn` — an assistant message: `at`, `phase`, `model`, its `tokens`
  (all five, so a per-turn in/out is one lookup), and how many `tool`
  events follow it. A turn's calls come directly after it, in the order the
  message listed them: the adjacency a consumer groups on.
- `tool` — one call, joined to its result: `tool`, `class` (the same
  read/edit/run/org/ask/delegate/other table the phase mix uses, so a
  consumer colouring by class agrees with the facts' `kind`), the
  `tool_use` `id`, `input_bytes`, `result_at`, `result_bytes`, `failed`,
  and the call's own file changes — `files`, `lines_added`/`lines_deleted`,
  `opaque` — under exactly the facts' two states.
- `phase` on every event: the index into the facts' `phases`. This is the
  join the WI asked for; prompts and questions are not repeated, since the
  facts already carry them with timestamps to merge on.
- `spawns[]`, one per `delegation.spawns[]`, each with its own events and
  no `phase` — a concurrent agent has no place on the parent's timeline.

Three rules decided along the way, each with a reason on the record:

- **Flags when true, absent otherwise.** `failed` and `opaque` are present
  only when true, so a clean readable call carries neither; `result_at` and
  `result_bytes` are absent together when no result arrived (an interrupted
  call, or one still running at the end of the transcript). The three-state
  "ok / failed / no result" therefore never needs a `null`.
- **A failure whose call is not in the file gets no event.** The facts count
  it under `<unknown>`; the events cannot place it, and inventing a
  synthetic call to hang it on would be the first invented thing in the
  document. The invariant is stated as `failed == tool_failures −
  <unknown>` instead. Zero such failures in the corpus; the fixture has one.
- **A record with no timestamp goes last, unplaced.** No bucket or phase
  can hold it, so its events carry neither `at` nor `phase` — kept, not
  dropped, because dropping them would break `tool events == tool_calls`.

**Sizes are honest about what they measure.** `input_bytes` is the input
re-serialized compactly with sorted keys — a canonical size, since the
on-disk bytes are not recoverable from a parsed value. `result_bytes` is the
UTF-8 length of the result's text *as the model was handed it*, which for
an offloaded result (11 in the kai corpus, ~24,400 results) is the
`<persisted-output>` placeholder and its 2 KB preview; the harness's own
`persistedOutputSize` sits beside it in `toolUseResult` and is documented
in `transcript-format.md` as the additive next step rather than carried
now.

**One pass, not two.** This is where #1617 pays off: `Counter::count`
builds the events as it counts, so the session, every spawn and the events
document come out of the same walk and cannot disagree. The events are
attached to the per-record tick that `activity` and `phases` are cut from,
so the phase stamp is the cut itself, not a re-derivation from timestamps.
The contract states seven invariants; a jq check over all 405 sessions
finds **0 violations** of any of them — tool events equal `tool_calls`,
turns equal `assistant_turns`, failed equals failures less `<unknown>`,
opaque equals `opaque_edits`, line deltas sum, per-phase counts match, and
per-spawn counts match. And the facts did not move: the events-tier binary
reproduces the #1616 sweep on **405 of 405**.

**`MAX_BUCKETS` stays at 240.** Revisited as the WI asked, and answered
rather than raised: the facts keep the resolution a static page needs, and
a consumer that wants finer buckets derives them from the events'
timestamps. Raising the ceiling would have grown every facts document for a
zoom only the app performs.

### #1618 — in-repo fixture, golden render test, and CI

The guarantees that mattered lived in sweeps against `/ai-data`, which a
clone cannot run, and nothing ran `just check` but whoever remembered.

**The fixture** (`tests/fixtures/root/`) is one hand-written session of 39
records and a 5-record sidecar — every line typed, none copied off the
volume — that carries one of every shape the docs name: the bare-string
prompt, the `<ide_opened_file>` sibling block, the slash-command scaffold
and its `isMeta` body, the `[Request interrupted]` placeholder, the resume
nudge that opens a span with nobody asking, a pasted image; `Edit` with a
`structuredPatch`, `Write` as a `create`, a failed `Bash`, an interrupted
`Bash` with no result, a `Bash` whose result was offloaded; both kaed
shapes; an answered `AskUserQuestion`; a joined `Agent` and an unjoined one;
a `Skill`; an org call; a `tool_result` for a call not in the file;
`"output_tokens_details": null`; an unknown record type; a CLI upgrade
mid-session; a two-hour gap. It yields two spans, five phases of three
kinds, 16 tool calls with two failures, and 4 files touched with 5 opaque
calls. `tests/fixtures/README.md` lists all of it.

**The goldens** (`tests/golden/`) are what kagviz produces from it — facts,
events, the report, the `sessions` table and the terminal `show`, the last
being the presentation layer that gets forgotten. `tests/golden.rs` runs
the *built binary* (`CARGO_BIN_EXE_kagviz`) rather than the library, so
`discover`, `load_facts`, `--from` through a file and through stdin,
`derive` over a scratch mirror and the index it writes are all on the path
— the untested CLI wiring the WI named. Nine tests. `KAGVIZ_UPDATE_GOLDEN=1
cargo test --test golden` rewrites them; the diff is the review surface.

**The probes.** Fourteen copy-pinned substring asserts and two whole tests
in `render.rs` — the ones that broke on wording edits without a behaviour
change: "an unknown, not a zero", "account for all of it", "Band colours
name the phase kind", "nothing here is inferred", "could not be joined",
"chose: SQLite" — are gone, held by the golden report instead. What stayed
is structural or numeric: the class names, the `1 failed · 25.00%`, the
`<strong>3</strong> combined`, the CSS mechanism the band labels rely on.

**CI**: `.github/workflows/check.yml` runs `just check` on push to `main`
and on pull requests — `dtolnay/rust-toolchain@stable` with rustfmt and
clippy, `Swatinem/rust-cache`, `taiki-e/install-action@just`. It could not
be exercised from this branch before the push, and the first run on the PR
failed in three seconds: the `just` installer it was first written with,
`extractor/setup-just`, does not exist. Fixed in-ship after checking the
replacement's repository and its `just` manifest through the GitHub API
rather than guessing a second time.

## The before/after, and what did not move

Against `a8dad05` (sprint 005), which `main` at e6355b9 reproduces exactly:

| | |
|---|---|
| #1617 | 405/405 byte-identical |
| #1616 | bytes change on 397/405; **no value moves**; strip the `null`s and dedup `subagents` in the baseline → 405/405 identical |
| #1615 | 405/405 identical to the #1616 sweep; the events are a new document |

New baseline for the sprint's facts at
`/ai-data/kagviz-data/baselines/<host>-2026-08-23/19a75d4/`, and the first
events snapshot beside it in `19a75d4.events/` — 199 + 93 + 113 of each,
produced by the binary built at that commit and checked against this
session's earlier sweeps before being written (0 differ, facts and events).

## Verified

- `just check` green throughout: fmt, clippy `-D warnings` over
  `--all-targets`, **92 unit tests** (87 at the start; 7 added, 2 retired
  into the golden) and **9 golden tests** through the binary.
- Corpus sweeps, 405 transcripts, 0 skipped lines on every one: the three
  rows above, plus the events invariants over every session.
- Rendered and looked at: the fixture's report and the events document,
  read through as a consumer would — the goldens in `tests/golden/` are
  those files.

## Follow-ups

- The corpus sweep is still a script in `.scratch/`; a `scripts/` or `just
  sweep` home for it would have saved an hour this sprint and will next.
- `persistedOutputSize` onto `tool` events, additively, when the app wants
  "what did this call really produce" beside "what did the model see".
- Front-end v1 (#1619) is unblocked: `sessions.json` → facts → events, all
  static, all linked.
