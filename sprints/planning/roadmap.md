# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records.

kagviz turns a session transcript into insight about how the agent worked.
The governing split: **everything countable is computed deterministically; a
model is used only to write the headline over facts already established.**

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

## Now

- **The headline pass.** Optional LLM labels for segments and a
  session-level one-liner, cached beside the transcript so a rendered report
  stays byte-stable once produced. Strictly additive: the facts never move.

## Next

- **Reconcile shell edits, honestly or not at all.** The `opaque_edits` gap is
  now the *only* undercount left, and it is the hard one: nothing in the
  transcript bytes can see a `sed -i`. Two separable pieces — narrow
  `opaque_edits` to shell calls that plausibly wrote (deterministic, from the
  command string), and a `git diff` figure that would have to be a separately
  named, clearly *inferred* field.

## Later / Ideas

- **Interactive report** — the real goal: pan the timeline, zoom from the whole
  session down to a phase, a turn, a single tool call. Forest, tree, leaf.
  Likely a TS front-end over the same facts JSON, which is why that JSON is
  treated as a contract from day one. The zoom half is now queued as #1591:
  sprint 002 confirmed that a wider page is *not* a substitute for it.
- Cross-session views: how a project's sessions trend over time.
- Compare two sessions side by side (the harness-eval use case).
- Feed reports to `ai-findings` as ready-made infographics.
- Consume `tool-results/*.txt` overflow for content-level analysis.
- Ship a hand-minimised, secret-cleaned fixture *in the repo*, so the corpus
  sweep is reproducible without access to `/ai-data`.
