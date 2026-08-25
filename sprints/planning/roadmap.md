# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records.

kagviz turns a session transcript into insight about how the agent worked.
The governing split: **everything countable is computed deterministically; a
model is used only to write the headline over facts already established.**

The destination, named at the MVP+ review (006): an **app** — bring up a
session from anywhere on the homelab, inspect it, dig in. Click a timeline
segment and see its records and tool calls, and which failed. The facts JSON
is the seam the whole plan hangs on, which is why it has been a contract
since day one.

## Shipped

- **Sprint 001 — static HTML report.** `kagviz render <session-id>` emits a
  self-contained HTML page from the facts JSON: tool mix with failures, file
  changes, a time strip collapsing idle, and markers where the user was
  involved. The facts contract grew `activity` and `user_involvement`, both
  additively. Verified on 305 transcripts across Linux and Windows; the same
  facts render byte-identically on both. See
  `sprints/001-static-html-session-report.md`.

- **Sprint 002 — phases, and a report that uses the screen.** The facts grew
  `phases`, additively: the session cut at user turns *and* idle breaks, each
  stretch labelled mechanically by its tool mix. The strip draws them as bands,
  the report gained a Phases panel, wall clock became active's sub-label rather
  than its peer, and the layout uses a wide monitor. Swept over 197
  transcripts. See `sprints/002-phases-and-wide-report.md`.

- **Sprint 003 — close the undercount.** An adapter table keyed by tool name,
  so an MCP file server's own unified diff is recovered instead of being
  invisible, and the subagent rollup: `subagents/agent-*.jsonl` folded in as a
  separate `delegation` tier with an explicit combined line. The facts grew
  `changes.by_tool` and `delegation`. The git-diff reconciliation was rejected
  rather than attempted — see the sprint record for why. Also pinned the first
  **regression corpora**: verbatim snapshots of kai, kubs0 and cleo under
  `/ai-data/kagviz-data`, 405 transcripts over CLI 2.1.176–2.1.240, with the
  facts each produced at a known commit. Zero parse failures, zero skipped
  lines. The Windows corpus paid for itself the same day, catching a defect no
  Linux transcript could have. See `sprints/003-close-the-undercount.md`.

- **Sprint 004 — the headline pass.** The first time a model is anywhere near
  the pipeline, and the deliverable is the seam more than the code: an opt-in
  `--label` writes a session headline and a label per phase, into a `labels`
  key that is the *only* model-written field and is absent without the flag.
  Never shown a number — the digest handed to the model carries ranked names,
  ordinal sizes and the user's own words, and nothing else, so it has no
  measurement to contradict. Cached on a `sha256` of the facts, so a labelled
  report re-renders byte-identically with the model host switched off. Proven
  additive against the pinned corpus: 405/405 sessions byte-identical to the
  sprint-003 baseline. See `sprints/004-headline-pass.md`.

- **Sprint 005 — the facts stop disagreeing with themselves.** The first
  breaking facts change since the contract was written, landed the way the
  contract demands: measured. `isMeta` records stop counting as prompts and
  slash commands start counting as the line the user typed (`user_prompts`
  2,012 → 1,831 over the corpus); `active_secs` is redefined as the sum of
  the span lengths, so `active_secs == Σ spans == Σ phases` holds by
  construction; a band too narrow for its label shows colour and tooltip
  instead of clipped garbage. Everything else byte-identical, and the sprint
  record names both halves. See `sprints/005-facts-stop-disagreeing.md`.

- **Sprint 006 — the MVP+ review.** First formal review:
  `sprints/review/006-mvp-plus-review.md`. Direction verdict: on course, and
  the two early bets (facts-as-contract, determinism + pinned baselines) are
  the reason. Named the four gaps between today's tool and the app — sessions
  don't survive the CLI's ~30-day prune, no browse surface, no detail below
  the bucket, no interaction layer — and queued the work below.

- **Sprint 007 — collect the fleet, nightly.** Transcripts self-prune at
  the source, so collection is what makes history exist. A live, accumulating
  mirror per host under `/ai-data/kagviz-data/live/` (kai by local rsync,
  kubs0 by rsync over ssh, cleo by rclone over sftp), never pruned and never
  written by kagviz; `kagviz derive` computes facts + report per new or
  changed session — by content hash, and in full when the kagviz version
  changes — and regenerates `sessions.json` (a second contract) and
  `index.html`; a systemd user timer on kai runs it at 04:00 Pacific; a host
  that does not answer is recorded as unreachable on the page, not read as
  "nothing new"; copyparty serves `derived/` on the tailnet at `/kagviz/`.
  See `sprints/007-collect-the-fleet-nightly.md` and `docs/collection.md`.

## Now

- **Report legibility quick wins (sprint 008).** #1590 (failure rate beside
  the count) and the #1591 interim: a zoom-in checkbox rendering dense strips
  at readable element size in a horizontal scroller — CSS `:checked` only, so
  the report stays self-contained with no JS.

## Next

- **The facts learn detail (sprint 009).** The contract work the app needs:
  a per-event detail tier (tool calls with name, timing, outcome) so a
  timeline segment can answer a click — leaning a *separate document* under
  the same contract discipline, so the summary stays light and detail loads
  on demand; the `MAX_BUCKETS` ceiling gets revisited here. Bundled with the
  contract hygiene the 006 review found, so one baseline regeneration covers
  it all: emit absent-not-null as the contract already promises, one shared
  accumulator for both counting tiers, and the in-repo fixture + CI + golden
  render test that make the guarantees reproducible from a clone.

- **Reconcile shell edits, honestly or not at all.** The `opaque_edits` gap is
  the one remaining undercount, and the hard one: nothing in the transcript
  bytes can see a `sed -i`. Two separable pieces — narrow `opaque_edits` to
  shell calls that plausibly wrote (deterministic, from the command string),
  and a `git diff` figure that would have to be a separately named, clearly
  *inferred* field.

## Later / Ideas

- **Interactive front-end v1** — the app itself: a TS SPA in `web/`, served
  static from the same host as the collected data, reading the session index
  → facts → events over HTTP with no backend. Timeline pan/zoom (#1591
  proper: forest, tree, leaf), click a segment for the drill-down. Queued as
  a WI; propose once 009 has landed and the events document's shape is
  proven — 007 shipped the index it reads first.
- Cross-session views: how a project's sessions trend over time — the
  collected `live/` store is what makes this possible at all.
- Compare two sessions side by side (the harness-eval use case).
- Feed reports to `ai-findings` as ready-made infographics.
- Consume `tool-results/*.txt` overflow for content-level analysis.
- Run `--label` in the nightly derive when kvllm is reliably up at 04:00
  (the derive takes the flag; the timer does not pass it yet), and judge the
  prose against a real model (sprint 004's open live-fire check).
