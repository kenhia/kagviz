# Sprint 013 — three facts corrections, one baseline regeneration

korg:1643 · #1640, #1647, #1653 · branch `013-three-facts-corrections`

## Goal

Three independent breaking changes to the facts, landed together so the pinned
corpus baseline is regenerated **once** instead of three times. That is the
whole argument for the bundle — sprint-005 economics — and it is the reason
these three, which have almost nothing to do with each other, share a sprint.

The proposal was titled "two" when the sprint opened; #1653 had been absorbed
into it the previous evening and the routing contract had not caught up.
Retitled at the start, because a queue row that misstates its own scope is
worse than no row.

## What was wrong, and how each was found

None of the three was found by a test. That is the finding behind the finding.

- **#1653 — one message, many records.** The harness writes one assistant API
  message as one record per content block (`thinking`, `text`, `tool_use`) and
  stamps every one with the same `message.id` and the *same* `message.usage`.
  `summary.rs` counted per record. Found in sprint 012 by reading the new
  segment panel against the transcript behind it: three consecutive rows saying
  `3,088 out` for what was one message.
- **#1647 — `<task-notification>` counted as the user.** When a background
  agent finishes, the harness writes a `type: user` record full of markup and
  flags it with **nothing** — no `isMeta`. Found while auditing what a demo of
  the browse page would put on a shared screen: harness XML where the user's
  own words belong.
- **#1640 — every shell call was an `opaque_edit`.** So "21,805 calls could
  have changed files" *was* the shell-call count. Most of them are a `grep`.

The first two are the same family, and it now has four members: `promptId`,
`isMeta`, `message.id`, `origin`. The tell is **a field that looks per-turn and
is really per-record** (or, for `origin`, per-*writer*). No test caught #1653
because the facts and the events count through one accumulator and both counted
it the same wrong way — every cross-check between them agreed. A cross-check
between two consumers is not a substitute for reading one against the source.

## Decisions

**The anchor first, before any code.** `main` at 76499a5 was swept over all 405
pinned transcripts and compared to the sprint-009 baseline `19a75d4`: **0 files
differ**. Sprints 010–012 moved no facts, checked rather than assumed, the same
way 009 checked `e6355b9` before it started. Every number below is stated
against that one anchor rather than against the change before it, because the
three corrections are independent and chaining them would make each unreadable.

**#1647: a deny-list of one, not an allow-list of `human`.** `origin.kind` is
the discriminator (`promptSource` is not — real input and notifications both
read `sdk`). Exactly two kinds exist, checked over the pinned corpus *and* the
413-session live mirror, across every record type. Two kinds do not justify an
allow-list, and the direction matters: under-counting the user is the same class
of lie as over-counting them, so a kind kagviz has never seen keeps being
counted. A third would surface the way this one did — as a prompt that reads
like markup.

The work item recorded the value as `"task"`; both corpora say
`"task-notification"`. Corrected on the item.

**#1653: dedup in the `Counter`, and `models` reads its decision.** The counter
is the one place a per-record quantity is read, so the session, every spawn and
the events document take the correction together. `models` is the exception that
proves the rule — it is counted in `summarize`, and now reads
`Counted::counted_turn` rather than deciding a second time. A quantity decided
in two places is a quantity that will eventually disagree with itself.

**#1640: an allow-list, and it is about *writing*, not about which files
matter.** The item asked whether git plumbing and build tools (`git commit`,
`cargo build` — 3,771 calls) should count as non-edits. **They stay opaque.**
`cargo build` runs `build.rs`; `git commit` writes `.git/`. Asserting they never
touch a tracked source would be kagviz claiming something it cannot see, and the
whole point of `opaque_edits` is not making claims like that. Keeping them
opaque needs no such claim, which makes it correct rather than merely cautious.

## What shipped

| | anchor `19a75d4` | shipped | sessions |
|---|---|---|---|
| `user_prompts` | 1,831 | **1,716** | 49 |
| `phases` | 3,802 | **3,687** | 49 |
| `assistant_turns` | 81,049 | **38,764** | 391 |
| `tokens.output` | 83,669,634 | **32,828,298** | 391 |
| delegated `assistant_turns` | 1,720 | **603** | 14 |
| delegated `tokens.output` | 500,833 | **91,502** | 14 |
| `changes.opaque_edits` | 21,821 | **15,391** | 296 |
| shell `opaque` / `calls` | 21,805 / 21,805 | **15,375** / 21,805 | 296 |

Named as precisely as what did move: `files_touched`, `lines_added`,
`lines_deleted`, `records`, `active_secs`, `wall_secs`, `idle_secs`, the span
boundaries, `phases[].records` and `phases[].mix` are byte-identical to the
anchor. `opened_by` moves on **0 of 405** sessions.

A structural field-by-field diff over all 405 pairs was run after each of the
three, which is what those "did not move" claims rest on rather than a summed
total agreeing by luck. The three surfaces barely overlap: #1647 moves the
user-side four, #1653 the assistant-side counts, #1640 only `changes`.

## The audit that earned its keep

The `#1640` classifier was written twice — a Python prototype to derive the
allow-list from what the corpus actually runs, then the Rust that ships. They
were diffed call by call over all 21,805 shell calls. **They disagreed on
exactly one**, and chasing that one call found two real holes:

- **A command-prefix wrapper allow-lists everything behind it.** `env` was on
  the list, and not harmlessly: **eight** corpus calls are `env -i … copilot`,
  `env -u … just publish`, `env HOME=… cargo`, every one judged read-only. The
  whole family — `env`, `command`, `sudo`, `timeout`, `nice`, `nohup`, `xargs` —
  is off it now. Price: two bare `env | grep` calls become opaque, which is the
  right way round.
- **A `{ … }` script block's body never reaches the classifier.** It becomes
  arguments of the cmdlet in front of it, so `Where-Object { Remove-Item $_ }`
  passed on `Where-Object` alone. A standalone `{`/`}` now refuses; brace
  *expansion* (`ls {a,b}.rs`) is one word and unaffected.

One call's disagreement was worth eight live misclassifications. Two
implementations of one rule is a cheap oracle, and worth repeating on any
classifier that decides what kagviz is allowed to claim.

An earlier draft also read *any* `>` as a file write and left 6,118 read-only
calls opaque. `2>&1` and `> /dev/null` are the overwhelming majority of `>` in
the corpus and neither writes anything.

## The defect this sprint created, and the warning it walked into

`opaque == calls` was the condition all three renderers used to mean "this tool
recovered nothing". #1640 breaks that proxy the moment a shell tool has calls
that are neither readable nor opaque — the fixture immediately started rendering
`Bash (0 file(s) +0/-0, 3 unreadable)`.

This is precisely the trap CLAUDE.md names from sprint 012: improving on a proxy
without first re-deriving what it was a proxy for. Re-derived rather than
patched. The report, the terminal `show` and the app's `Panels.svelte` now
assemble the line from what is actually known — recovered counts first, opaque
count second, "nothing written" when there is neither. Every pre-existing
rendering is byte-identical; only the two new cases are new.

## Verification

`just check` is green. Beyond it, over all 405 sessions and every spawn:

- **The events invariants hold.** `turn` events == `assistant_turns`, `tool`
  events == `tool_calls` summed, opaque events == `opaque_edits`, per-phase
  `tool_calls` and `output_tokens` add up, and `by_tool` sums back to the
  totals with no tool showing `opaque > calls`.
- **Adjacency survives a split message.** The contract promises a `turn` is
  followed directly by its `tool` events; after #1653 a message's calls can
  arrive two records after the turn opened. It holds because the records
  between carry no events of their own — but that was checked, not assumed, and
  it is the one property the fixture alone could not have proved.
- **0 skipped lines, 0 parse failures**, every sweep.

Three premises behind #1653, all re-checked over the pinned corpus rather than
carried over from the live mirror the discovery was made on: the usage block is
byte-identical across all 42,285 continuation records; a message's records are
contiguous (0 exceptions — the implementation still keys on `message.id`, so an
interleaving would dedup rather than double-count); and every assistant record
carries an id (0 without, so the count-on-its-own fallback is unit-test-held,
on the same footing as `isSidechain`).

## The baseline

Written at `b0edcaa` under `/ai-data/kagviz-data/baselines/<host>-2026-08-23/`,
facts and a matching `b0edcaa.events/`, 405 of each. It round-trips (0 files
differ against itself) and differs from `19a75d4` on 391 sessions. The four
older baselines are untouched — a baseline's value is saying what was true
then, so they are never overwritten. The corpus `README.md` carries the
before/after table and now points its regression-check snippet at `b0edcaa`.

**The end-to-end check, on the session the work item named as worst.**
`a811ca00` in `hv-simulator` had been reporting **5,560 turns / 6,628,363
output** on its report and its session page. With the shipped binary it reports
**2,908 / 2,704,095** — exactly what #1653 predicted, arrived at independently
of the corpus aggregate. The same session shows #1640 too: 1,004 shell calls,
773 of them opaque, 231 judged read-only.

## Fixture

It now carries one of each new shape, which is what it is for: a
`<task-notification>` record; a `message.id` on **every** assistant record,
because that is what the corpus looks like; a split message in the session tier
and another in the subagent sidecar, so the golden proves the delegated tier
dedups through the same accumulator. Its four `Bash` calls already split
1 read-only / 3 opaque without being touched.

## Follow-ups

- The sweep tooling still lives in a git-ignored `.scratch/013/` and is copied
  forward from 009 each sprint. A `scripts/` home has been a noted follow-up
  since 009 and is now three sprints old.
- `docs/transcript-format.md` still says *last checked 2026-08-23 against 405
  transcripts*, and the corpus is four days older than the live mirror it was
  cut from. A refresh is not urgent; a newer corpus would be a new pinned
  directory, not an overwrite.

## Deployed

Pending — `deploy-kagviz`, which sprint-ship runs in Phase 7 from merged `main`
(there is no `just deploy` recipe; the skill drives `just web-deploy` and then
`just collect-derive`, and the latter depends on `build-release`, which is what
actually replaces the binary the 04:00 timer runs).

**One thing that inverts for this sprint.** The skill's step 6 proof is that *a
sprint which did not change the extractor must move zero derived bytes* — sweep
`sha256sum` over `facts/`, `events/` and `reports/` before and after, and diff.
013 is the opposite case: it should move **nearly every one of those 1,221
documents**, and a small diff would be the alarming outcome. Until that runs,
the browse page and the app are serving pre-013 numbers — `assistant_turns` and
every token figure roughly 2× and 2.5× what they should be.

Expect roughly 391 of 405 pinned-corpus sessions' worth of churn, scaled to the
live tree's 413. The pinned-corpus before/after in **What shipped** above is the
prediction to check the deploy against.
