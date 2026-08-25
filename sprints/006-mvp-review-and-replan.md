# Sprint 006 — MVP+ review and next-phase plan

korg:1611 · covers #1610 · branch `006-mvp-review-and-replan`

## Goal

Ken's brief at POC + 5 sprints: the usual maintainability review, but the
primary question is **"are we moving in the right direction for this
tool"** — toward an app where a session can be brought up, inspected, and
dug into. Then, with the review in hand, plan the next work: roadmap,
work items, proposals. A review-and-planning sprint that ships records, not
code.

Two threads Ken named going in: **interactivity** (click a timeline segment
→ the records and tool calls behind it, which failed) and — the priority —
**collection scaffolding** (new/updated sessions from kai, kubs0 and cleo
into `/ai-data/kagviz-data`, manual + nightly ~0400 PT, preprocessing folded
into the nightly run).

## What shipped

**The review**: `sprints/review/006-mvp-plus-review.md` — the first record in
`sprints/review/`. Short version:

- **Direction verdict: on course.** The two bets made before the app existed
  — facts-as-contract and determinism + pinned baselines — are exactly what
  the app needs, and nothing shipped so far has to be unwound. Four named
  gaps separate the tool from the app: sessions don't survive the CLI's
  ~30-day prune, no browse surface, no detail below the bucket, no
  interaction layer. Those four gaps *are* the plan.
- **One bet to stop doubling down on**: the static renderer. Legibility
  tweaks only from here; interaction belongs to the front-end the contract
  was built for.
- **Maintainability: boundaries hold** (renderer never touches the
  transcript, the label pass is never in the path of a number, `summarize`
  is a pure function of pre-read bytes — each verified with evidence, not
  asserted). Five ranked risks, the sharpest being real contract drift:
  the doc says absent, the serializer emits `null`, on every optional field
  except `labels`. Cheap to fix now, expensive after the first external
  consumer.

**The designs**, written into the review at planning depth:

- *Collection* (thread 1): accumulating live mirror per host under
  `/ai-data/kagviz-data/live/` beside the untouched pinned corpus; sync that
  never propagates deletions; a derive stage (facts, reports, cross-host
  `sessions.json` + `index.html`, stamped with the kagviz commit); a systemd
  user timer on kai at 04:00 America/Los_Angeles with the units authored in
  this repo; serving via the existing tailnet-only copyparty. Feasibility
  settled during the review: kai owns the volume, rsync covers kai/kubs0,
  cleo answers ssh from kai (the pull tool is the sprint's decision), and
  kai's timezone makes 04:00 exactly Ken's 0400 PT.
- *Interactivity* (thread 2): three horizons — the CSS-only zoom checkbox
  now, the events detail tier as the next contract surface (leaning a
  separate document so the leaf loads on demand; `MAX_BUCKETS` revisited
  there), then a static TS SPA with no backend, reading the same served
  files. No korg program: everything lives in this repo, and the proposal
  queue's ranks do the sequencing.

**The plan, filed in korg:**

| korg | What | Covers |
|---|---|---|
| 1620 | Sprint 007 — collect the fleet, nightly (rank 1) | #1612 mirrors + timer, #1613 derive + browser |
| 1621 | Sprint 008 — report legibility quick wins (rank 2) | #1590 failure rate, #1614 zoom checkbox |
| 1622 | Sprint 009 — the facts learn detail, contract gets honest (rank 3) | #1615 events tier, #1616 absent-not-null, #1617 one accumulator, #1618 fixture + golden + CI |
| — | #1619 front-end v1 (XL), queued unproposed; depends on #1613 and #1615 | |

#1591 (zoom) stays open as the want; a comment there routes its two halves
to #1614 and #1619.

**In the repo**: the roadmap rewritten around the app destination (and the
missing sprint-005 Shipped entry restored — found by this review);
CLAUDE.md gained the "adding a facts field touches up to six places"
checklist, the one review finding cheap enough to fix in place.

## Decisions

- **Collection before interactivity.** Ken's call, and the review sharpened
  the reason: the source data self-prunes, so every month without collection
  is a month of sessions that stop existing. Nothing else in the plan has
  that property.
- **Small code fixes deferred to #1617, not snuck in here.** The proposal
  says this sprint ships a review, not code; the stale `#[allow]`, the
  overstated render.rs doc comment and friends are named in a WI instead of
  quietly landing in a "docs" PR.
- **No korg program.** Single repo throughout; programs are the
  cross-project layer. The recorded trigger for revisiting: `web/` becoming
  its own repo.

## Follow-ups

- The review's serving note is a standing decision for Ken: reports carry
  prompt previews, and copyparty's access control is tailnet-only with no
  accounts — same boundary as `~/src` today, but said out loud in #1613 so
  it is accepted rather than inherited.
- 0400 was specified with a "(?)" — the timer lands at 04:00
  America/Los_Angeles unless Ken says otherwise on korg:1620.
