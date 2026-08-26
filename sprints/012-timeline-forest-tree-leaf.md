# Sprint 012 — front-end v1, part 2: the timeline — forest, tree, leaf

korg:1642 · covers #1591, #1639 · second half of #1619

## Goal

The half sprint 011 deliberately left out: pan and zoom the timeline from the
whole session down to a turn, and click any piece of it for the turns and tool
calls behind it. #1591 has been open since sprint 002 — *"what is needed is to
be able to zoom in on the timeline"* — and everything since has been building
the thing it needed: phases to zoom **to** (002), the events document to zoom
**into** (009), and an app that could hold interaction at all (011).

It closes #1619 and #1591.

## One decision, made at the start, that shaped everything else

The proposal describes three levels — whole session, one span at bucket
resolution, a phase at turn resolution. **It is built as one continuous zoom
instead**, with breadcrumb buttons that pick a scale rather than a mode.

The axis makes it work: x is **active seconds with idle collapsed**, which is
the coordinate the strip already used and the reason it reads at all. So a
track is

```
width = Σ span.secs · pxPerSec  +  breaks · breakPx
```

and `pxPerSec` is the entire zoom control. Panning is a scroll offset. "Forest,
tree and leaf" are three neighbourhoods of one scale, not three renderings —
and *"a column is a turn"* falls out of the axis at deep zoom rather than
needing a fourth layout. Snapping to a level is then just a button that sets
`pxPerSec`, which is how `whole session`, `this stretch` and `this phase` work.

## What shipped

**`timeline.ts`** — the geometry, pure. Track, resolution, columns, bands,
marks, ticks, hit-testing, and the inverse used to anchor a zoom under the
cursor. **`segment.ts`** — what a click resolves to, and the merge of the two
documents behind it. Both fully tested against the repo's goldens; 90 tests
where 011 left 83.

**`Timeline.svelte`** replaces `Strip.svelte`, which is deleted. Two
implementations of one panel is how they drift, and everything the strip knew
— the three break densities and the measured reason for them — carried over.

**`Segment.svelte`** — the panel: counts from both tiers, then the rows, turns
holding their own tool calls, prompts and questions merged in by time, files
expandable with ± lines.

**A spawn is a selection too**, opened from the delegated tier's row rather
than from the timeline — it is not on the timeline, because phases cut the
*parent's* timeline and a spawn's events carry none. It is checked against
`delegation.spawns[k]` instead of a phase, and **nothing the user said is
merged into it**: `user_involvement` is the parent's, and a subagent has no
user. The row offers no door until the events are read.

**The selection lives in the hash.** `#/s/<host>/<id>?phase=3`, or
`?span=0&from=120&to=150`. Read on arrival, framed by the timeline, written
back on every change.

## The bar means two different things, and the caption always says which

`bucket_secs` is a property of the **session**: the facts resolve the series at
that width and no finer. So a column is a whole number of facts buckets summed
— exact, and identical to the report's strip — until the zoom goes past it, at
which point there is nothing finer in the facts and the column is re-bucketed
from the **events**.

Those two are not the same number. Above `bucket_secs` the bar counts
`records`; below it, turns and tool calls, because that is what the events
carry. `records` includes `system` and snapshot records, which is exactly why
the contract says the events do not reproduce it. The caption names the
resolution, the document and the metric on every frame, and the legend agrees
with it.

**The fit view is always the facts**, even once the events are here. It is the
report's strip, it is what the page draws before the big document lands, and a
panel that silently changes what its bars count the moment a fetch completes is
the thing the caption exists to prevent. Refining is something the reader does,
by zooming. That was got wrong first: the events arrived and a short session's
default view quietly became turns-and-calls at 2s.

## Two findings, and both were about a claim, not about code

### The contract's per-phase failure invariant was false, and three places asserted it

#1639's rule is *"the panel's counts must equal the facts' counts it was opened
from — a mismatch is a bug, never something to round."* Rendering both tiers
side by side forced the question of **which** quantities are actually equal.
Not all of them are:

- the facts count a `tool_failures` on the record carrying the **result**
  (`summary.rs`, the `tool_result` branch);
- an event carries `failed` on the **call**, stamped with the call's own `at`.

A call whose result came back after a boundary is counted on one side of it and
drawn on the other — **in either direction**, so a phase can place *more*
failures than it counts.

Measured over the live tree's 413 sessions:

| per phase | disagreements |
|---|---|
| `tool_calls` | **0** |
| `output_tokens` | **0** |
| `tool_failures`, events > facts | **17 phases** |
| per-session total shortfall ≠ `<unknown>` | **0** |

Calls and tokens are genuine equalities; failures hold only in sum across the
phases. The claim lived in three places — `docs/facts-contract.md`,
`tests/golden.rs` and the app's `conformance.spec.ts` — and held in all three
only because `fixture-0001` has no straddling call. `tests/golden.rs` would
have **underflowed** a `u64` rather than merely failed. All three corrected;
**no value moved and no emitted document changed a byte.** What changed is what
the text promises.

Same shape as the defect 011's conformance test found on its first run, one
layer deeper. The panel therefore separates a **disagreement** (calls or tokens
differ — a defect, rendered as a warning) from a **note** (failures differ —
the two documents counting different records, said in words).

### One assistant message is written as several records, all with the same usage

Reading the panel against the transcript behind it: three consecutive rows
saying `claude-opus-4-8 · 3,088 out`. The records are one message,
`msg_013aTUq198`, split into `thinking` / `text` / `tool_use`, each stamped
with the same `message.usage`.

`summary.rs` counts per record. Over the live mirror's 408 sessions with
assistant records:

| quantity | as counted | actual | error |
|---|---|---|---|
| `assistant_turns` | 82,416 | 39,343 | **+109.5%** |
| `tokens.output` | 87,992,219 | 33,570,698 | **+162.1%** |

403 of 408 sessions (98%). This session's own headline — *5,560 turns,
6.6M out* — is really 2,908 messages and 2.7M.

Filed as **#1653** and bundled into sprint 013 (korg:1643), which is already a
baseline regeneration: one regeneration rather than two. Not fixed here — it is
a facts change that moves nearly every number kagviz reports, and this sprint is
the interaction layer. Written up as trap 6 in `docs/transcript-format.md`,
where the next person to touch the extractor will meet it.

No test could have caught it: **both** documents count the same wrong way, so
every cross-check between them agreed. It took a person reading a panel next to
the file it came from — which is the argument for the panel existing, made on
its first day.

## Three defects found by looking, again

The ratio held from 011: the tests were green and the screen was wrong.

1. **The axis read `19:19` eight times across.** Ticks are chosen at ≥96px
   apart, which at one second per column is a five-second step — and `clock()`
   renders `HH:MM`. It is not a clock, it is noise where a clock should be.
   Seconds now appear once the step is finer than a minute.
2. **Ticks were generated for the whole track and filtered afterwards** — about
   two million objects a frame at leaf zoom on a twelve-hour session. `ticks()`
   takes the window and walks only the part of each span inside it.
3. **A 3px break between wide columns stopped reading as a separator.** The one
   thing a break has to say is that time was removed here — 53 minutes, between
   two columns sitting a few pixels apart. So a break grows with the columns…

…and **that fix reproduced the defect 011 shipped**, which is the part worth
recording. At fit, 208 breaks at a column's width claim more than the whole
viewport, every span is squeezed to nothing, and the panel whose job is
collapsing idle renders only idle — the exact failure the three densities were
introduced to fix, arrived at from the opposite direction. The densities were a
proxy for the *share of the visible width* the breaks take; `breakWidth` now
bounds that share directly (`BREAK_SHARE_MAX = 0.45`, from the report's own
measured 47%), with the densities as the floor. There is a test on 2, 13, 61
and 209 spans.

## Performance, on the corpus's hardest session

`a811ca00` — 209 spans, 390 phases, 2,777 tool calls, **2.6 MB of events**:

- facts on screen in **99 ms**; events read and decoded at **1.03 s**, with
  their size on the page while they arrive. Nothing above the timeline waits
  for them.
- The track reaches **1,023,489 px** at full zoom. One `<svg>` is positioned at
  the scroll offset with a `viewBox` in track coordinates, so **227 rects** are
  in the DOM there — the visible slice and a screen of margin either side.
- One click handler, not one per rect. Hit-testing goes through `locate()`;
  tens of thousands of focusable elements would be worse for a keyboard than
  what is there.
- No page errors, no failed requests, no horizontal page scroll at 1400px,
  760px or 390px, light and dark.

## What is deliberately not here

- **A transcript viewer.** The panel shows what happened — turns, calls, sizes,
  files — not what was said inside them. That is a different, later thing, and
  the events document says so where it lists what it does not carry.
- **Zoom and pan in the hash.** Only the *selection* is deep-linked; arriving
  with one frames it. A link that also pinned a scroll offset would break the
  moment the window was a different width.
- **#1653.** Sprint 013, with the baseline regeneration it needs.
