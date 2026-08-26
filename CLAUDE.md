<!-- kproject:begin — managed by kprojects; do not edit inside this block -->
## kproject conventions

This project uses the kproject minimal harness
(<https://github.com/kenhia/kprojects>). Keep context small; prefer doing
over ceremony.

### Layout

- `sprints/` — the project's evolution, one record per PR-sized unit of
  work (a "sprint")
  - `planning/` — planning docs; at minimum `roadmap.md` (the general plan)
  - `review/` — more formal reviews as the project matures
  - sprint records: `###-<short-name>.md` for small projects, or a
    `###-<short-name>/` directory of files for larger/more formal ones
  - a sprint record is one informal narrative: goal, decisions, what
    shipped, follow-ups — written during the sprint, not after
  - projects that deploy end the record with a `## Deployed` section:
    what shipped, where, when, and what was verified live — appended
    after the deploy, not predicted before it
- `docs/` — project documentation, architecture, usage
- `.scratch/` — git-ignored scratch space for user or agent ephemera;
  use it instead of /tmp
- `justfile` — dev recipes; default recipe is `@just --list`; `just check`
  runs the CI gates; `just deploy` (or variants) if the project deploys
- `.env` — git-ignored; tokens and environment vars

### Workflow

- One sprint ≈ one PR. Sprint proposals and work items are managed in
  `korg`; durable cross-project knowledge goes in `klams`.
- Mark each work item resolved as its work completes — don't batch the
  resolutions into sprint-ship. A proposal's progress should be readable
  while the sprint is running, which is the only time it is useful.
- If the korg or klams MCP tools are unavailable in your session, say so
  up front — don't silently work around missing infrastructure.
- A few projects share contract surfaces with siblings and have a
  **guiding plan** constraining how those change; most have none, and one
  grep is the whole cost of finding out. Grep the `index.md` routing
  table in `kai:~/src/tools/cross-project-planning` — a local path on
  kai, read through kaed from any other host (`root: "kai:src"`, path
  `tools/cross-project-planning/…`); don't clone a second copy. Not
  listed → nothing applies. Listed → read the mapped plan folder before
  planning sessions and before changing a contract surface it names, and
  amend the plan in the same ship when what you build diverges from it.
- TDD preferred: write the failing test first when practical.

### Tooling preferences

- Rust managed by `cargo`; format with `cargo fmt`, lint with
  `cargo clippy --all-targets` (test targets included deliberately — a gate
  that skips them is a gate that lies)
- Mirror `rust-toolchain.toml`, `rustfmt.toml` and `clippy.toml` from a
  sibling homelab repo rather than generating them
- License is MIT unless specifically directed otherwise
<!-- kproject:end -->

## Project

kagviz reads Claude Code session transcripts and reports how a session went —
tool mix and failures, file changes, where time went, where the user was
involved. The audience is the person who asked for the work, not someone
debugging the agent.

Early-stage: the deterministic core works and `kagviz render` emits a
self-contained HTML report. Roadmap in `sprints/planning/roadmap.md`.

### The rule that governs the design

**Facts are computed, never inferred.** Everything in `summary.rs` is a pure
function of the transcript bytes, so the same session yields the same numbers
forever. A model may later write a *headline* over those facts — segment
labels, a one-line summary — but it must never be in the path that produces a
number. If you find yourself wanting to ask a model what happened, you are
writing the wrong layer.

The facts JSON (`kagviz show <id> --json`) is a **contract**. A future
interactive front-end consumes it. Changing its shape is a breaking change;
adding to it is not.

### Build and test

```sh
just check    # cargo fmt --check + clippy --all-targets -D warnings + test
just fmt
```

`just check` is the real gate — `rust-check` (fmt, clippy `-D warnings` over
test targets too, tests) **and** `web-check` (prettier/eslint, svelte-check,
build, vitest). Run it before shipping, not just `cargo test`. CI runs the same
recipe (`.github/workflows/check.yml`).

The app's half includes the **contract conformance test**, which decodes the
checked-in goldens and asserts the invariants `docs/facts-contract.md` states.
That is the point of it being in this gate rather than a separate one: a facts
change that breaks the front-end fails the Rust build the day it lands. If you
move a golden, read the conformance test's failure as carefully as the golden
diff — the first time it ran it found a defect in the *contract text*, not in
the code.

`tests/golden.rs` runs the built binary over the hand-written fixture under
`tests/fixtures/` and compares every output — facts, events, report, the
`sessions` table, the terminal `show` — byte for byte with `tests/golden/`.
When a change moves one on purpose, `KAGVIZ_UPDATE_GOLDEN=1 cargo test
--test golden` rewrites them; **read the resulting diff** — it is the review
surface for any change to a document or the page.

### Read these first

- `docs/transcript-format.md` — the on-disk format and its traps. **Read this
  before touching the extractor.** It is field-derived, not documented
  upstream, and the format drifts between CLI releases.
- `docs/facts-contract.md` — the JSON `show --json` emits, and the rules for
  changing it — plus the two documents under the same rules, `sessions.json`
  and the events document (`show --events`). **Read this before adding or
  renaming a field** — and note what the events do *not* promise: per phase,
  `tool_calls` and `output_tokens` add up, `tool_failures` does not. The facts
  count a failure where its result came back and an event carries `failed` on
  the call, so a call whose result crossed the boundary lands on one side and
  is drawn on the other. Corrected in 012 after three places asserted it.
- `src/transcript.rs` — tolerant record model. Parsing must never reject an
  unknown record type or field.
- `src/summary.rs` — the deterministic pass.
- `docs/collection.md` — the live mirror under `/ai-data/kagviz-data/live`,
  the nightly `derive`, what is served, and **deploying** — the artifact is
  `target/release/kagviz` in this checkout, because that is what the timer
  runs. **Read this before touching `collect/` or `src/derive.rs`.**
- `web/README.md` — the app: why the router is hash-based, why the bundle
  lives under `derived/`, and the two traps that only show up on deploy.
  **Read this before touching `web/`.**
- `sprints/planning/roadmap.md` — what is planned and why.

### Conventions that are easy to get wrong

- **A record is not a turn, and `promptId` does not mark a user prompt.** Two
  traps of one family: fields that look per-turn and are really per-record.
  `promptId` groups every record in a turn, tool results and harness-injected
  text included — use `is_user_turn`. And one assistant *message* is written as
  several records, all carrying the same `message.usage`, which is why
  `tokens.output` is inflated 162% today (trap 6, #1653). Read
  `docs/transcript-format.md` traps 1, 5 and 6 before touching it — not `INJECTED_PREFIXES` alone, which is only the
  narrow half. The load-bearing half is **`isMeta`**: the harness flags what it
  wrote, and that beats matching the shape of it. `<command-*>` is emphatically
  **not** on the prefix list — it is structure, and `command_line` reads the
  user's typed line back out of it.
- **Never report an unknown as a zero.** Shell-based edits leave no recoverable
  diff; they are counted as `opaque_edits`, not folded into the line deltas. A
  number kagviz cannot see must be visibly absent, not silently zero.
- **Report active time alongside wall time.** Either alone misleads — resumed
  sessions span days and hold minutes of work.
- **`null` is not `default`.** `#[serde(default)]` covers an absent field, not
  a present `null` — and a rejected field takes the whole record with it. Any
  typed non-`Option` field needs `deserialize_with = "null_as_default"`.
- **The renderer reads the facts, never the transcript.** If you find yourself
  wanting a value the facts do not carry, add it to the facts. The index
  (`sessions.json`, `index.html`) reads the facts files the same way — and is
  its own contract, documented beside the facts. So does the app in `web/`,
  which is now a **third** consumer of the same documents: a rule the report
  follows (written text marked, unknowns visibly absent, no quantity the
  contract does not let a consumer recompute) has to hold there too, and where
  the report already solved a display problem — the strip's break densities are
  the worked example — port the solution rather than rediscovering it. Sprint
  012 is the cautionary half of that: porting the densities was right, and
  *improving* on them without first re-deriving what they were a proxy for
  reproduced the exact defect they had been introduced to fix.
- **Nothing writes into `live/<host>/projects/`.** The mirrors are verbatim;
  everything computed goes under `derived/`, stamped with the kagviz that made
  it, and is regenerable. A sync never propagates a deletion.
- When adding a field parsed for future use, annotate it and say what will read
  it. Use `#[expect(dead_code)]` where nothing reads it yet, `#[allow(...)]`
  where only tests do (an `expect` would be unfulfilled under `--all-targets`).
- Validate extractor changes against **real transcripts** under
  `~/.claude/projects`, not only the unit tests. Every trap documented so far
  was found that way.
- **Both tiers and the events count through one `Counter`.** `Counter::count`
  is the only place a per-record quantity is read — the session, every spawn
  and the events document all come out of it, so they cannot disagree. A
  quantity counted in `summarize` alone reaches neither the delegated tier
  nor the events; if it is a count, put it in the `Counter`.
- **Adding a facts field touches up to eight places** — walk the list, because
  the missable one is different every time: (1) `Summary` + the `summarize`
  loop, or the `Counter` if it is a per-record count; (2) `Spawn`, if the
  delegated tier should carry it too — a quantity in one tier and not the
  other makes them silently non-comparable; (3) `docs/facts-contract.md`,
  always; (4) the report in `render.rs`; (5) the *terminal* view in
  `main.rs::show_session`, the third presentation layer and the one that
  gets forgotten; (6) the goldens under `tests/golden/`, regenerated and
  read; (7) `web/src/lib/contract/` — the type, the decoder, and the panel
  that shows it, since sprint 011 — and `Segment.svelte` too, since 012, if a
  consumer would compare the field against the events (the conformance test
  will not fail for a field the app merely ignores, which is exactly right and
  is also why this one is easy to skip); (8) the corpus sweep, to prove the
  change additive
  (or to measure it, if it is not).
