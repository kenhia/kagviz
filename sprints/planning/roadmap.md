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

## Now

- **Timeline segmentation.** Cut phases at user-turn boundaries and label each
  segment deterministically by its tool mix (Read-heavy → exploring,
  Edit + test → implementing). This is the skeleton the interactive view
  needs, and it must exist before any model gets involved. The activity series
  from 001 is what it segments over.

## Next

- **The headline pass.** Optional LLM labels for segments and a
  session-level one-liner, cached beside the transcript so a rendered report
  stays byte-stable once produced. Strictly additive: the facts never move.
- **Per-tool diff adapters.** Close the `opaque_edits` gap where possible —
  kaed edit results carry their own diffs, and a `git diff --stat` over the
  session window can reconcile shell edits.
- **Subagent rollup.** Fold `subagents/agent-*.jsonl` into the parent session's
  facts, so delegated work shows up as work rather than as a single tool call.

## Later / Ideas

- **Interactive report** — the real goal: pan the timeline, zoom from the whole
  session down to a phase, a turn, a single tool call. Forest, tree, leaf.
  Likely a TS front-end over the same facts JSON, which is why that JSON is
  treated as a contract from day one.
- Cross-session views: how a project's sessions trend over time.
- Compare two sessions side by side (the harness-eval use case).
- Feed reports to `ai-findings` as ready-made infographics.
- Consume `tool-results/*.txt` overflow for content-level analysis.
- Schema-drift regression corpus: pin one transcript per CLI version and assert
  the extractor still reads them.
