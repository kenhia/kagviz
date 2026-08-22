# 001 — Static HTML session report

**Proposal:** korg:1547 · **Work items:** #1537, #1538, #1539, #1541

## Goal

Turn the working facts extractor into something Ken can look at and react to.
The deterministic core landed with the scaffold, but nothing rendered it —
and a report you can argue about is the cheapest way to find out whether the
facts are the right facts, before committing to an interactive view.

## What shipped

`kagviz render <id>` writes one self-contained HTML file.

- **`src/render.rs`** — the report. Session identity, a headline stat row, the
  time strip, tool mix with failures, file changes, token totals, user
  involvement, delegation.
- **`src/summary.rs`** — two additive extensions to the facts contract:
  `activity` (the bucketed time series) and `user_involvement` (the ordered
  decision points).
- **`src/fmt.rs`** — shared duration and count formatting, so a duration never
  renders two different ways between the terminal and the page.
- **`docs/facts-contract.md`** — new. The JSON shape and the rules for
  changing it.

25 tests, `just check` green, validated against 305 real transcripts.

## Decisions

**The renderer reads facts, never a transcript.** `render` takes `&Summary`
and nothing else. `--from facts.json` and `--from -` (stdin) exist so the seam
is exercised in anger rather than merely asserted, and a test locks it: a
report built from a *serialized* facts document must be byte-identical to one
built from the summary in memory. If that ever drifts, the seam a future
front-end plugs into has quietly stopped being real, and this is the alarm.

**`bucket_secs` belongs to the session, not the renderer.** The activity
series picks its own column width from a fixed ladder (5s → 1800s), the
narrowest that keeps the whole series under 240 buckets. Had the renderer
chosen it, two renderings of one session could disagree about the scale —
precisely the drift the determinism rule exists to prevent. Across all 305
transcripts the ladder never bottomed out (worst case 239 buckets).

**Idle occupies no buckets.** Spans are cut at `IDLE_GAP_SECS` and carry
`idle_before_secs`; the renderer collapses what the facts already separated.
The definition of idle lives in one place.

**Buckets carry counts, not meaning.** Records, tool calls, failures, user
turns, output tokens — no classification of what the work *was*. Segmentation
is 002's job and a label in a bucket would be the wrong layer.

**An unanswered question stays unanswered.** `chosen: null` renders as "no
answer recorded" rather than defaulting to the first option. Same instinct as
`opaque_edits`: an unknown must be visibly absent, never a plausible value.

## What the corpus taught us

Two things came out of running against real transcripts that no unit test
would have found.

**A `null` where a number belongs was dropping whole records.** One CLI
version writes `"output_tokens_details": null`. `#[serde(default)]` covers an
*absent* field — a present `null` is still handed to the field's deserializer,
which rejects it, and the rejection is not scoped to the field: serde rejects
the **record**. So the line was skipped and that turn's tool calls, timestamp
and model vanished along with its token counts, with nothing in the output
looking wrong. Fixed with a `null_as_default` deserializer across the usage
fields and `message.content`, plus regression tests, and written up as trap #4
in `docs/transcript-format.md`. Corpus-wide skipped lines: 1 → 0.

It cost nothing to find only because the sweep asserts *zero* skipped lines
rather than "few". That threshold earned its keep on its first outing.

**`AskUserQuestion` answers are structured, not prose.** #1539 assumed the
choice would have to come out of the `tool_result` content — which is a
formatted English sentence with the answers interpolated. The record's
`toolUseResult` actually carries an `answers` object keyed by the question
text and valued with the chosen label. Join on `tool_use_id`, then on the
question string; no prose parsing. Documented.

## Verification

- **kai**: 197 transcripts, 0 parse failures, **0 skipped lines**.
- **cleo (Windows)**: 108 transcripts across 11 CLI versions (2.1.209 –
  2.1.238), 0 failures, **0 skipped lines**. `USERPROFILE` fallback confirmed
  by removing `HOME` and re-running. Drive-path project slugs (`D--ClaudeWorks`)
  and backslash `cwd` (`D:\ClaudeWorks`) both handled. Transcripts are written
  **LF even on Windows**, so CRLF was a non-issue — the reader trims anyway.
- **Cross-platform determinism**: the same facts document renders
  **byte-for-byte identically** on Linux and Windows (51,512 bytes, `cmp`
  clean). Facts extracted on Windows, rendered on Linux, same file.

cleo's Rust install is broken (`cargo.exe` is a 0-byte file), so rather than
install a toolchain there, the binary was cross-compiled on kai for
`x86_64-pc-windows-gnu` and copied to `%TEMP%`. Nothing was installed on cleo.

The hardest case in the corpus is session a811ca00 (hv-simulator): 54 days
wall, 12h39m active, **209 spans**. That is the session the time strip exists
for, and it reads as work rather than as whitespace.

## Follow-ups

- **cleo's Rust toolchain is broken** — `C:\Users\kenhi\.cargo\bin\cargo.exe`
  is 0 bytes. Cross-compiling worked around it for this sprint, but a repair
  or a documented cross-compile recipe is worth a work item.
- **Very long unbroken spans could still exceed 240 buckets.** A span cannot
  contain a gap of 120s or more by definition, but a dense enough session
  could in principle run past the ladder's 1800s top rung. Never observed
  across 305 transcripts; the strip scrolls if it happens.
- **Subagent transcripts are counted but not folded in** (#1545). The report
  says so on the page rather than letting the totals read as complete.
- The report is the thing to argue with now. Whether the right panels are
  there is the question sprint 002 should open with.

**From Ken's review of the MVP report** — both filed, neither shipped here:

- **#1557 — de-emphasize the wall clock.** It sits as a peer of active time in
  the headline row at equal weight, and on a 54-day session the eye lands on
  the bigger, less meaningful number. Keep it on the page (trap #2 exists
  precisely because active-alone is dishonest) but not at the same billing.
- **#1558 — use the width on a wide screen.** `max-width: 1000px` is right for
  the prose panels and wrong for the time strip, which is the one panel where
  horizontal room is strictly more information — columns floor at 2px and
  start scrolling well before a wide monitor runs out of space.
