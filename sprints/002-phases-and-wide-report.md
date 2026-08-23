# 002 — Phases, and a report that uses the screen

**Proposal:** korg:1584 · **Work items:** #1542, #1557, #1558

## Goal

Give the session a spine. Sprint 001 shipped a time strip that shows *when*
work happened; this one cuts that strip into labelled stretches so it also
shows *what kind* of work it was — and does it deterministically, before any
model is anywhere near the page. Bundled with Ken's two notes on the MVP
report, both of which land on the same panel.

## What shipped

**`phases` in the facts** (`src/summary.rs`), additive — nothing above it
moved. Each phase carries its window, the span it lies in, a mechanical
`kind`, and the `mix` of tool classes that `kind` was computed from.

**Phase bands on the time strip** and a Phases panel (`src/render.rs`): a
rollup of where the time went by kind, then the ordered narrative under it.

**The headline row** folds wall clock into active's sub-label, and the layout
uses the width it has.

`docs/facts-contract.md` documents the new field and the rules for it.
36 tests, `just check` green, swept over 197 real transcripts.

## Decisions

**A phase is cut at user turns *and* at idle breaks.** The proposal asked for
this to be decided explicitly rather than discovered later, and the answer is
both. User turns because that is where the work was redirected. Idle breaks
because a phase that ran across a gap would report a three-day pause as its own
duration — the wall-clock lie one level up, which is exactly the thing 001
built spans to avoid. So `span` is a field on every phase, and a phase that
resumes after a break has no `opened_by`: nobody asked for it.

**A phase runs until the next one starts, not until its own last record.** The
forty seconds between an agent's last tool call and the user's next turn are
real work. Giving them to neither phase would have left the durations quietly
short with nothing on the page to show it.

**Labels name a tool mix, not an intent.** `implementing` means files were
edited here. `running` means mostly shell — and under agent instructions that
prefer shell editing, that may well *be* editing kagviz cannot see, which is
why it is not called "verifying". The descriptive layer is #1543 and it is a
different field. This one must never start sounding like it.

**Thresholds are integer percentages in `summary.rs`.** Same argument as
`bucket_secs`, one step further: two renderings of one session must not
disagree about what a phase was, and a float comparison is one platform
difference away from making them.

**Band geometry stayed in the renderer.** The facts say what a phase is and
when; where its band lands on the strip is layout, and the renderer already
does that arithmetic for the user-involvement marks. Keeping it out kept the
contract to what a future front-end actually needs.

**Width was the wrong fix for the right complaint, and that is worth saying.**
#1558 asked the report to use a wide screen; it now does. But looking at the
result beside a short session, Ken's read was that width is not what the strip
needed — *zoom* is. A long session is a wall of 2px columns at any page width,
and the detail only becomes readable at the small end. The layout change stays
because it reads well at half-screen width, which is how Ken works much of the
time, but the thing that actually hurts is filed as #1591.

**`MAX_BUCKETS` stayed at 240.** #1558 flagged it as something a wider strip
might justify raising. It is a facts change that would move `bucket_secs` on
every session, so it is not something to do as a CSS side effect. Untouched.

**Wall clock kept its number and lost its tile.** Ken picked the first option
on #1557. The headline now reads `19m active · over 2h02m wall · 1h43m idle` in
one tile. Trap #2 is satisfied — wall is still on the page, and is no harder to
find than idle — but the eye lands on the number that answers "how much work
was this".

## What looking at the rendered page taught us

Three things came out of Ken screenshotting the report that neither the tests
nor the corpus sweep would have surfaced, because all three are about how the
page *reads*.

**The strip that collapses idle was rendering 72% idle.** Breaks are a fixed
width, and at 209 stretches the 208 breaks claimed 1248px while the work
columns were squeezed onto their 2px floor and the strip scrolled. A break only
has to be visible — its width deliberately does not encode duration — so above
sixty stretches it now narrows to 3px with its dashed borders dropped. The
strip stops scrolling, idle falls to about 45% of the rendered width, and the
columns roughly double.

**Wall clock read as `1297h15m`.** `fmt::duration` topped out at hours, so the
one number #1557 moved into the sub-label to keep it legible was the least
legible thing on the page. It now reads `54d01h`. The rung is shared, so
multi-day idle breaks and the terminal output improved with it.

Ken's call on where to stop, which is worth keeping because it bounds the
whole panel: at the density where the breaks become noise however thin they
are, the strip is already past what any static rendering can fix, and only
zooming it will help. So 3px is not a compromise pending a better number —
below it, the work belongs to #1591.

**The phase bands are invisible on that session.** Each span is a few pixels
wide, so every label clips to nothing — this sprint's headline feature,
unreadable on exactly the session that needed it. Narrowing the breaks helped
and did not fix it; filed as #1594.

The through-line: the corpus sweep proves the numbers, and it cannot see a
single one of these. Rendering a hard session and *looking* at it belongs in
the verification list next to the sweep.

## What the corpus taught us

**Phase durations did not add up, and the page said they did.** The unit tests
used whole-second timestamps and passed. Real transcripts carry milliseconds,
and `num_seconds()` floors: truncating each phase's own duration lost under a
second per phase, which is three seconds a span and 216 seconds across the
392-phase, 209-span hv-simulator session. Meanwhile the report was asserting
in prose that the durations added up to active time.

Fixed by measuring both ends as whole-second offsets from the span start and
subtracting, so the truncations telescope and the phases in a span sum to
exactly that span's length. There is a regression test with millisecond
timestamps, because no whole-second test would ever have caught it.

The residual against `active_secs` is **not** fixed and is older than this
sprint: `active_secs` is `wall_secs - idle_secs`, two truncations, while the
spans truncate once each. 198s out of 12h39m on the worst session, 0 on most.
Filed as #1587 rather than changed here — it moves a shipped field, and the
sprint's own notes warn against making a facts change as a side effect.

**The label distribution is a mirror.** Across 197 sessions: 137 with
`running` phases against 15 with `exploring`. That is not a bad classifier, it
is the shell-first agent instructions showing up in the data — the same effect
`opaque_edits` measures, seen from another angle. Worth remembering before
anyone "fixes" the thresholds.

**163 of the hv-simulator session's 392 phases are shorter than one column** at
that session's 1800s bucket width, so the strip cannot draw them. The page says
so and the rollup still counts them. Same instinct as `opaque_edits`: what the
page cannot show, it says it cannot show.

## Verification

- **197 transcripts on kai**, 0 parse failures, **0 skipped lines** (unchanged).
- 1,997 phases produced. Every one: valid `span` index, non-negative duration,
  `ended >= started`, and **phases tile their span exactly** — 0s discrepancy
  across every span in the corpus.
- Hardest case unchanged: a811ca00, 209 spans, now 392 phases, 12h39m active
  over 54 days. Renders in 212KB with 229 bands drawn.
- `just check` green: fmt, clippy `-D warnings` over `--all-targets`, 38 tests.
- Read on screen, not only asserted: the hardest session and a short one were
  rendered and looked at. That is where the three rendering defects above came
  from, and it is worth doing every sprint that touches the page.

An earlier sweep reported `ended < started` on two sessions. That was the
sweep's own jq comparing RFC 3339 strings of differing fractional precision
(`10:00:00.5Z` sorts before `10:00:00Z`), not a defect. Re-checked with parsed
datetimes: zero.

## Follow-ups

- **#1587 — `active_secs` and the span lengths disagree by up to 198s.** Filed
  above.
- **#1591 — zoom the timeline.** The real answer to the complaint #1558 was
  standing in for. Blocked on a decision, not on effort: the report is
  self-contained with no JS by rule, and zoom is interaction. Either the TS
  front-end the facts contract exists for, a static approximation that
  pre-renders a second scale, or relaxing the no-JS rule — which is its own
  decision and not a side effect of wanting zoom.
- **#1594 — phase bands unreadable at high span counts.** The label lives
  inside the band, and a 4px band holds no word. Cheapest honest fix is to drop
  the label below a threshold and keep the colour and the tooltip.
- **#1590 — tool failure rate beside the count.** XS, and it lives in the same
  `headline()` this sprint rewrote. Left out because it arrived as feedback on
  the finished work, not as scope. The denominator is `total_tool_calls`: a
  failed call is a call, counted once.
- **`discussing` is noisy on long sessions.** 194 of hv-simulator's 392 phases
  are `discussing`, holding 15 minutes between them — mostly one-record spans.
  Honest, and the rollup makes the proportion obvious, but if the phase list
  ever becomes the primary view it will want collapsing.
- **The band is per-bucket, so phase boundaries snap to columns.** At 1800s
  buckets that is a coarse approximation of where a phase actually started.
  An interactive view would draw them at true positions; the static strip
  cannot without leaving the flex layout.
- The phase list caps at 30 and then shows the 15 longest. Fine for a static
  page, wrong for the interactive one.
