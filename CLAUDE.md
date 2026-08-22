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

Early-stage: the deterministic core works, the HTML report does not exist yet.
Roadmap in `sprints/planning/roadmap.md`.

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

`just check` is the real gate — run it before shipping, not just `cargo test`.
Clippy runs with `-D warnings` over test targets too.

### Read these first

- `docs/transcript-format.md` — the on-disk format and its traps. **Read this
  before touching the extractor.** It is field-derived, not documented
  upstream, and the format drifts between CLI releases.
- `src/transcript.rs` — tolerant record model. Parsing must never reject an
  unknown record type or field.
- `src/summary.rs` — the deterministic pass.
- `sprints/planning/roadmap.md` — what is planned and why.

### Conventions that are easy to get wrong

- **`promptId` does not mark a user prompt.** It groups every record in a turn,
  tool results and harness-injected text included. Use `is_user_turn`, and see
  `INJECTED_PREFIXES`. This one has already been got wrong twice.
- **Never report an unknown as a zero.** Shell-based edits leave no recoverable
  diff; they are counted as `opaque_edits`, not folded into the line deltas. A
  number kagviz cannot see must be visibly absent, not silently zero.
- **Report active time alongside wall time.** Either alone misleads — resumed
  sessions span days and hold minutes of work.
- When adding a field parsed for future use, annotate it and say what will read
  it. Use `#[expect(dead_code)]` where nothing reads it yet, `#[allow(...)]`
  where only tests do (an `expect` would be unfulfilled under `--all-targets`).
- Validate extractor changes against **real transcripts** under
  `~/.claude/projects`, not only the unit tests. Every trap documented so far
  was found that way.
