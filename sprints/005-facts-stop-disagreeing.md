# Sprint 005 — the facts stop disagreeing with themselves

korg:1606 · covers #1605 (skill invocations counted backwards),
#1587 (`active_secs` vs the span lengths), #1594 (unreadable phase bands)

## Goal

Three fixes that share a cost rather than a theme. #1605 and #1587 are both
**breaking** changes to the facts, and each one needs a regenerated baseline
over a 405-transcript, 568 MB corpus. Shipping them apart means doing that
twice and writing two before/after diffs. Together it is one.

And this is the **first breaking facts change since `docs/facts-contract.md`
was written down**, so part of the job is showing what that process looks
like: state the numbers that moved, and — just as precisely — the numbers that
did not.

## What shipped

### #1605 — a slash command writes two records, and kagviz kept the wrong one

```jsonc
// 158 bytes, no isMeta. This is the user. kagviz DISCARDED it.
{"type":"user","message":{"content":
  "<command-message>start-sprint</command-message>\n
   <command-name>/start-sprint</command-name>\n
   <command-args>korg:1606 proceed with implementation</command-args>"}}

// 7 KB, isMeta: true. This is the harness. kagviz COUNTED it as the prompt.
{"type":"user","isMeta":true,"message":{"content":
  "Base directory for this skill: …\n # Start Sprint Skill\n …"}}
```

The first record was on `INJECTED_PREFIXES` as "slash-command scaffolding".
The second matched no prefix, so it became `user_prompts`, `opened_by`, the
preview, and — because phases cut at every user turn — **a phase boundary that
should not exist**.

Two halves, and they turn out to be disjoint (no `<command-*>` record in the
corpus carries `isMeta`, so neither fix is hiding a mistake in the other):

1. **Parse `isMeta` and exclude it.** The load-bearing half. The harness
   *flags* what it wrote, which beats matching the shape of it, and the flag
   is a strict superset of several prefixes that were already on the list —
   798 `isMeta` user records in the corpus, 113 of which the prefix list was
   already catching.
2. **Take `<command-*>` off the prefix list and parse it.** It is *structure*,
   not noise. `command_line()` reads the tags by name and reconstructs
   `/start-sprint korg:1586 procceed with implementation` — exactly, typo and
   all, which no amount of prefix-stripping the 7 KB document would achieve.

`Base directory for this skill:` needed nothing done to it: it is derivable
from the skill name, and the facts already carry a `skills` list.

Traps the corpus supplied, all of them in `a_command_scaffold_is_read_by_its_tags_not_its_layout`:

- **Both tag orders occur.** `/model` writes `<command-name>` first; skills
  write `<command-message>` first. Read tags by name, never by position.
- **Later lines carry the emitting command's indentation.** Read tags, not
  layout.
- **`<command-args>` is sometimes empty (`/clear`) and sometimes absent
  (`/exit`).** Both mean "no arguments"; neither is a parse failure.
- **`isMeta` is written as an explicit `false` as often as it is omitted.**
  `Option<bool>` through `unwrap_or(false)` — reading it as present/absent
  would have been wrong on 71 records in `~/.claude/projects` alone.

### #1587 — `active_secs` is now the sum of the span lengths

`active_secs` was `wall_secs - idle_secs` — two truncations — while the spans
truncate once each. On the 209-span session they disagreed by 198 seconds out
of 12h39m.

Three options were on the table and the choice was made on the record rather
than by default:

| | verdict |
|---|---|
| **Sum of the span lengths** | **Chosen.** Phases already tile their span exactly (002), so `active_secs == Σ spans == Σ phases` becomes true *by construction*. |
| Compute from ms, truncate once | Rejected. More accurate in absolute terms, but the spans still truncate individually, so `Σ spans ≠ active_secs` remains. It fixes the magnitude, not the defect in the title. |
| Leave it, keep documenting | Rejected, but legitimately arguable at 0.43% worst case. |

It is read off `s.activity.spans` rather than recomputed, so the identity
cannot drift later. A spawn's `active_secs` takes the same definition applied
to its own timestamps (`active_from_stretches`) — the delegated tier has no
strip to read spans off, and "active" has to mean one thing to be worth
printing side by side.

### #1594 — a band too narrow for its label shows colour and tooltip instead

A band a few pixels wide clipped its label to garbage, so on exactly the
session phases were built to make legible the band row degraded to an
unreadable stripe.

The fix is the work item's cheapest option, done properly: **the label is
gated on the band's rendered width**, via a container query. That width is not
a fact about the session — it falls out of the flex share, the window and the
2px column floor — so only the browser knows it. The markup always carries the
label; the stylesheet decides whether to show it. A browser without container
queries keeps the default and reads the same way, which is the intended
fallback rather than a degradation.

`BAND_LABEL_MIN_PX` is sized for the *longest* label, so a label that is shown
is never a clipped one. The number is spelled in the `@container` rule and
mirrored in Rust only so a doc comment and a test can hold the two in step.

Below the bar, two things already carried the kind and still do: the band's
colour and its tooltip. What was missing was a **key on the card the reader is
looking at**, so the Time card now says where the colours are named. Without
that, dropping the label would have traded clipped garbage for an
undecipherable rainbow.

## The before/after, and what did not move

Swept both binaries over the pinned corpus — 405 transcripts, 3 hosts, **0
parse failures, 0 skipped lines** on each side.

```
                    76daf8a        a8dad05
user_prompts          2,012   →      1,831
phases                4,003   →      3,802
active_secs         560,283s  →    558,730s
```

Decomposed, because the net figure hides both halves of #1605:

| | |
|---|---|
| harness bodies no longer counted as prompts | **−685** |
| real slash-command inputs recovered | **+504** |
| net | −181 |

`active_secs` moved on 305 of 405 sessions, **every one downward** (the old
figure was systematically long), worst case −198s on `kai/a811ca00` — exactly
the number #1587 was filed with.

Everything else is byte-identical: `wall_secs`, `idle_secs`, `assistant_turns`,
`pasted_attachments`, `ask_user_questions`, all five token counts, `tool_calls`,
`tool_failures`, all four `changes` counts, and the span boundaries themselves.
Only `activity…buckets[].user_turns` moved inside `activity`, which is #1605
arriving where it should.

Baseline written to `/ai-data/kagviz-data/baselines/<host>-2026-08-23/a8dad05`.

**Sprint 004 moved no facts, and that was checked rather than assumed** —
`76daf8a` reproduces the sprint-003 baseline `3823617` byte for byte across all
405 sessions. Worth knowing: it means the labels really are the sandboxed layer
they were built as, and it gave this sprint a trustworthy anchor to diff from.

## Two things the proposal got wrong

Both were reasonable and both were wrong, and saying so is cheaper than
leaving a future reader to trust the estimate.

**The proposal predicted #1605 would remove ~41 spurious cuts (~10% of phases)
from `a811ca00`. It removed 2.** That session is an hv-simulator build with
almost no skill invocations; its 209 spans come from idle cuts. The
*sequencing* argument survives untouched — #1605 does not dissolve #1594, and
#1594 still had to come second because it tunes against phase counts that
moved corpus-wide — but the magnitude was off by a factor of twenty.

**The proposal's baseline of "1,753 counted user prompts, 684 of them
`isMeta`" did not reproduce.** Direct measurement puts the before figure at
2,012. The two numbers that mattered landed exactly: 504 real inputs recovered,
and 113 `isMeta` records already caught by prefix.

## Knock-on, as predicted

Every sprint-004 cached label invalidates, correctly — the labels described a
session whose phases and prompts have since changed, and `facts_digest` is
what notices. First live demonstration that the cache's invalidation rule
works.

## Verified

- `just check` green: fmt, clippy `-D warnings` over `--all-targets`, 76 tests
  (5 new).
- Corpus sweep, both binaries, 405 transcripts: 0 parse failures, 0 skipped
  lines; every moved field inside the intended surface, nothing outside it.
- **Rendered and looked at**, which is how #1594 was found in the first place.
  `a811ca00` before/after: the band row goes from a stripe of clipped slivers
  to clean colour, and the headline reads 12h36m / 390 phases / 185 prompts.
  A 3-span session confirms the good case did not regress — `mixed`,
  `implementing` and `running` still label themselves, and the one sliver band
  correctly shows nothing. Its phase list now reads `1m — /model opus[1m]`: a
  recovered user input, rendering as the line that was typed.
