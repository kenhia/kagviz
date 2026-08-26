# Sprint 010 — CI off the deprecated Node 20 runtime

korg:1632 · covers #1631

## Goal

Every run of the workflow sprint 009 added prints:

> Node.js 20 is deprecated. The following actions target Node.js 20 but are
> being forced to run on Node.js 24: actions/checkout@v4.

One pin. The sprint exists as much for the method as for the fix: this is
the first of a fleet-wide chore, and Ken's call was a skill rather than a
standing instruction in the global `CLAUDE.md` — a one-time fix per repo
should not be a context tax on every session. The method here is the worked
example that skill (`agent-skills`, `gha-runtime-bump`) generalises.

## What shipped

`actions/checkout@v4` → `@v7`. Nothing else in the workflow needed to move.

## How the pin was chosen — the method

1. **The CI log is the authoritative list.** The runner names every action
   targeting the deprecated runtime, nested ones included. Here: one.
2. **Pre-flight every `uses:` through the API**, not memory: `runs.using`
   from `action.yml` at the pinned ref.

   | pin | `runs.using` | verdict |
   |---|---|---|
   | `actions/checkout@v4` | `node20` | bump |
   | `Swatinem/rust-cache@v2` | `node24` | already fine (v2 floats) |
   | `dtolnay/rust-toolchain@stable` | `composite` | no runtime of its own |
   | `taiki-e/install-action@just` | `composite` | no runtime of its own |

3. **Read the later majors' release notes for breaking changes**, against
   how this workflow actually uses the action (defaults; `push` and
   `pull_request` triggers):

   | major | runtime | what changed | touches us? |
   |---|---|---|---|
   | v5.0.0 (2025-08) | node24 | the runtime bump; runner ≥ 2.327.1 | no (hosted runners at 2.336.0) |
   | v6.0.0 (2025-11) | node24 | credentials persisted to a separate file | no |
   | v7.0.0 (2026-06) | node24 | fork-PR checkout blocked under `pull_request_target` / `workflow_run`; ESM | no — neither trigger is used |

   Latest compliant major wins, so the pin is not touched again for a
   while; the fallback, had v6 or v7 broken an input in use, would have
   been the lowest `node24` major (v5).
4. **Prove it on the run**, not by inspection: the PR's own CI log must no
   longer carry the notice.

## Verified

- `just check` green locally (no Rust changed; the gate is the rule).
- PR #10's CI run (`32926897570`) green in 21s, and its log carries **zero**
  `Node.js 20 is deprecated` notices and no other runner warnings — checked
  by reading the log, which is the only proof that counts. Sprint 009's
  runs carried one on every job.

## Follow-up

The skill that generalises this: `agent-skills` korg:1633, with a handoff
carrying the method and this sprint as its worked example.
