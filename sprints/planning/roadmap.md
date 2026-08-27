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

- **Sprint 008 — report legibility quick wins.** Two presentation fixes Ken
  pointed at on the 002 report, and no fact moved: 191 of 194 local sessions
  render byte-identical facts to main's, the other 3 having been resumed since
  the nightly. The headline tools tile reads `45 failed · 1.62%` — over the
  calls, a failed call being a call, with `<unknown>` failures kept out of the
  numerator so the rate cannot pass 100% — and the terminal `show` says the
  same through the same `fmt::percent`. A zoom-in checkbox above the time
  strip, CSS `:checked` only, sets every column to 12px and lets the strip
  scroll: offered from 120 columns, where the full-width layout starts
  squeezing them, and it only ever widens. The zoom exposed the row of clipped
  timestamps under dense strips, now gated on rendered width the way band
  labels are. The real pan/zoom (#1591) still waits for the front-end. See
  `sprints/008-report-legibility-quick-wins.md`.

- **Sprint 009 — the facts learn detail, and the contract gets honest.** The
  events document: `kagviz show <id> --events` (and `derived/events/`, linked
  from every `sessions.json` row) carries every turn and tool call with
  sizes, outcome, the files each call changed and the phase that holds it —
  a separate document under the same contract rules, built in the same pass
  as the facts so the two cannot disagree; seven stated invariants hold on
  405/405 corpus sessions, and `MAX_BUCKETS` stays 240 because finer buckets
  are now derivable from the events. The facts stopped emitting `null` for
  absent optional fields (397 of 405 sessions' bytes, zero values moved) and
  `subagents` became the set it was documented as; both tiers count through
  one `Counter`; a hand-written fixture with five goldens driven through the
  built binary, and CI running `just check`. Front-end v1 is unblocked. See
  `sprints/009-facts-learn-detail-contract-gets-honest.md`.

- **Sprint 010 — CI off the deprecated Node 20 runtime.** One pin:
  `actions/checkout@v4` → `@v7`. The sprint exists as much for the *method* —
  the CI log is the authoritative list, every `uses:` is pre-flighted through
  the API rather than from memory, and the proof is the run's own log — which
  is what the `gha-runtime-bump` skill generalises. See
  `sprints/010-ci-off-node-20.md`.

- **Sprint 011 — front-end v1, part 1: the skeleton, the contracts in
  TypeScript, the session browser.** A SvelteKit + Svelte 5 + TypeScript
  static SPA in `web/`, hash-routed, served from `derived/app/` on the data's
  own origin and deployed at `/kagviz/app/index.html`; the browser sorts and
  filters 407 sessions across three hosts, and a session page carries the
  report's panels over the same facts document with the time strip drawn once
  as SVG. The three contracts are typed in `web/src/lib/contract/` with a
  conformance test over the repo's goldens that runs inside `just check` and
  CI — the app is now the contract's second consumer *in the gate*, and on its
  first run it found the events document's per-phase invariant stating one
  fewer carve-out than it needed (`<unknown>` failures land in a phase and
  cannot be placed in the events). Sprint 012 found the rest of that story: the
  per-phase failure rule was never true at all, in either direction. `just check` grew a `web-check` half and CI grew Node. The static
  report is unchanged. No interaction yet. See
  `sprints/011-front-end-v1-skeleton.md`.

- **Sprint 012 — front-end v1, part 2: the timeline — forest, tree, leaf.**
  Pan and zoom, and the click into a segment. Built as **one continuous zoom**
  rather than the three levels the proposal described: the axis is active
  seconds with idle collapsed, so `pxPerSec` is the whole control and forest,
  tree and leaf are neighbourhoods of one scale. Above `bucket_secs` a column
  is whole facts buckets and the bar counts `records`; below it the column is
  re-bucketed from the events and counts turns and tool calls, and the caption
  names which is on screen — the fit view is always the facts. 2.6 MB of
  events read in 1.0s on the corpus's hardest session, a 1,023,489px track
  with 227 rects in the DOM. Closes #1619 and #1591.

  Two claims turned out to be wrong, and neither was code. The contract's
  **per-phase failure invariant is false** — the facts count a failure where
  its result came back, an event carries `failed` on the call, so a call whose
  result crossed the boundary lands on one side and is drawn on the other (17
  phases of 413 sessions; `tests/golden.rs` would have underflowed a `u64`).
  Corrected in the document and both tests; no value moved. And **one
  assistant message is written as several records carrying the same usage**, so
  `tokens.output` overcounts by **162%** and `assistant_turns` by **109%** on
  98% of sessions — filed as #1653, bundled into 013, and written up as trap 6
  in `docs/transcript-format.md`. Found by reading the new panel against the
  transcript behind it; no test could see it, because both documents count the
  same wrong way. See `sprints/012-timeline-forest-tree-leaf.md`.

## Now

Front-end v1 (#1619) is done: it was split in two because its halves carried
different risks — part 1 was plumbing, part 2 the interaction — and part 1
shipped useful on its own, which is what let part 2 be about the interaction and
nothing else.

Sprint 013 is the facts catching up, and it is the largest correction the
project has made. Three breaking changes, one baseline regeneration
(`sprints/013-three-facts-corrections.md`): `assistant_turns` and every token
figure were counting **records** where the harness writes one message as
several — `tokens.output` 83.7M → **32.8M** over the pinned corpus, 391 of 405
sessions; `<task-notification>` records were being counted as the user
speaking; and `opaque_edits` was every shell call rather than the ones that
could have written.

The pattern behind all three is worth keeping in view: **every one was a field
that looks per-turn and is really per-record**, and none was found by a test.
#1653 came out of reading sprint 012's own segment panel against the transcript
behind it; #1647 came out of asking what a demo would put on a shared screen.
The facts and the events agreed throughout, because both counted the same wrong
way — which is exactly why a cross-check between two consumers is not a
substitute for reading one against the source.

## Next

- **Sprint 014 — `just demo`** (korg:1649; #1648). One recipe to show kagviz to
  someone who is not on the tailnet: a curated session tree, the app deployed
  into it, an audit of what it would put on a shared screen, served over plain
  LAN HTTP. copyparty binds loopback only — the tailnet-IP listeners on its
  port are `tailscaled` — so there is no LAN path today, and the live tree's
  413 sessions of prompt previews are the real exposure, which is a curation
  problem rather than a hosting one. Packaging only.

- **Sprint 015 — the leaf opens: read what the call actually said**
  (korg:1659; #1656, #1657, #1658). Sprint 012's leaf tells you a `Bash` call
  ran, took 4s, sent 90 bytes, got 4,400 back and failed. It cannot tell you
  what the command *was* — the events document carries no payload text on
  purpose, because "this document would be the transcript again". A fourth
  served document (`calls/<host>/<id>.json`) carries each call's input and
  result, and the segment panel expands a row into it.

  **Measured before proposing, and it inverted the premise.** This was expected
  to need a server on the host for the data volume. It does not: the harness
  already bounds payloads by offloading large results to `tool-results/*.txt`
  and leaving a `<persisted-output>` placeholder, so across 413 sessions the
  90,182 payloads total **103 MB** — median 308 B, p99 11.8 KB, **max 85 KB**,
  and *nothing* over 100 KB. Per session that is a 190 KB file at the median
  against an events document the app already fetches at 42 KB. Same class of
  artefact, not a new one.

  What *would* justify an engine is the cross-session work under Later/Ideas —
  trends across a project, comparing two sessions, search — where a query
  ranges over all 413 and cannot be answered by fetching one file. Building a
  service for the leaf would cost the property that makes this cheap:
  `derived/` is disposable and the deploy artifact is one binary a timer runs.

  **The gate is disclosure, not storage.** Everything served today is derived,
  and the raw mirrors are unreachable. Call text would be the first raw session
  content on a served surface — and 59 of 413 sessions carry a
  credential-shaped payload (~51 plausibly live, plus 172 placeholder DSNs).
  A redactor is the obvious move and the trap: one that catches 51 and misses
  the 52nd manufactures false confidence, which is the unknown-rendered-as-a-
  zero this project refuses everywhere else. So #1657 is a decision, taken
  first, on the record.

  Ranked behind 014 deliberately: `just demo` has the same exposure problem one
  order of magnitude smaller and is likely to build the curation this reuses.
  Flip the order if it does not.

## Later / Ideas

- Cross-session views: how a project's sessions trend over time — the
  collected `live/` store is what makes this possible at all.
- Compare two sessions side by side (the harness-eval use case).
- Feed reports to `ai-findings` as ready-made infographics.
- Consume `tool-results/*.txt` overflow for content-level analysis.
- Run `--label` in the nightly derive when kvllm is reliably up at 04:00
  (the derive takes the flag; the timer does not pass it yet), and judge the
  prose against a real model (sprint 004's open live-fire check).
