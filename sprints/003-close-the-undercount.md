# Sprint 003 — close the undercount: shell edits and subagent work

korg:1585 · covers #1544 (per-tool diff adapters), #1545 (subagent rollup)

## Goal

The report systematically understates the sessions that did the most work.
Two causes, one shape:

- A session that edits through the shell or through an MCP file server shows
  near-zero files changed, because only `Edit`/`Write` expose a
  `structuredPatch`.
- A session that delegated heavily shows **one** `Agent` tool call where
  dozens of tool calls actually happened.

Both are extractor-side, both are additive to the facts, and both turn on the
same question: **how does a document whose credibility rests on never
rendering an unknown as a zero report a quantity that is exact, inferred, or
invisible?**

## The answer, given once

Every file-change quantity is **sourced**, and a source is in exactly one of
two states:

1. **Recovered** — an exact number, read out of a diff the transcript
   actually carries. Contributes to `lines_added` / `lines_deleted` /
   `files_touched`.
2. **Unrecovered** — a call that could have changed a file and exposed
   nothing readable. Counted in `opaque_edits`, never folded into the deltas.

There is no third state in the shipped facts. **Inferred numbers are not
shipped** — see "What we rejected".

`changes.by_tool` is the audit surface that makes this checkable rather than
believed: per tool, how many edit-capable calls there were, what was recovered
from them, and how many stayed opaque. It is the same argument `mix` makes for
a phase's `kind` — carry the counts the label came from.

The subagent tier is the same rule applied to work rather than to lines:
delegated work is reported as its **own tier** with an explicit combined line,
never silently merged into the parent's totals. Burying delegated cost inside
the parent hides the number a reader most wants to see.

## What the corpus taught us

Swept 199 transcripts on kai. (The cleo corpus is not reachable from this
host; anything below that depends on Windows-only shapes is called out.)

**`mcp__kaed-*__edit` returns its own unified diff.** `toolUseResult` is a
*JSON string*, not an object:

```json
"{\"applied\":true,\"diff\":\"--- a/x.md\\n+++ b/x.md\\n@@ …\",
  \"files\":[{\"path\":\"x.md\",\"new_version\":\"…\"}],\"txn_id\":21}"
```

Exactly recoverable. 7 such calls in the corpus, and **none of them were being
counted at all** — not in the deltas and not in `opaque_edits`, because
`may_edit_opaquely` only ever named `Bash` and `PowerShell`.
`docs/transcript-format.md` claimed MCP editors landed in `opaque_edits`; the
code never did it. That is the one place an existing field's value moves.

**`file-history-delta` is a dead end, and it is worth writing down.** The
record type looks like exactly what the shell-edit gap needs — the harness
tracking every file it saw change, with `trackingPath` and a backup
reference — and 1,610 of them sit in the corpus. Measured: joining
`trackingPath` against `cwd` (or `backup.realParentDir` when the file is
outside `cwd`), the set of files it names is a strict **subset** of the files
`structuredPatch` already covers. Tracked-but-not-patched across the whole
corpus: **0**. It is a backup-before-`Edit` marker, not a filesystem watcher.
It cannot see a shell edit. Recorded in `docs/transcript-format.md` so nobody
spends the afternoon again.

**Subagent transcripts are self-identifying, and the undercount is brutal.**
`subagents/agent-<agentId>.jsonl`; records carry `agentId` *and* the parent's
`sessionId`. One spawn in the corpus (`a3f518e638b914e3e`) ran **48 tool
calls and 25k output tokens** while the parent reported a single `Agent` call.

**No `isSidechain: true` records anywhere in the kai corpus.** The older
inline format is real but not present here, so the code path that handles it
is written from the format's shape and covered by unit tests, not by a corpus
measurement. Said plainly rather than implied.

Thin corpus warning, stated rather than buried: only **3** `Agent` calls
across **2** sessions on kai. The rollup's unit tests carry more weight than
the sweep does for this half.

## What shipped

Facts, additively except where noted:

- `changes.by_tool` — per-tool `{calls, files_touched, lines_added,
  lines_deleted, opaque}`.
- `delegation` — `{spawns[], unjoined_spawns, inline_records, totals}`.
- **Moved on purpose:** `changes.*` on the 3 transcripts carrying kaed edits,
  which were previously invisible in both directions.

## What we rejected

**Git-diff reconciliation of shell edits.** The proposal scoped it as the
risky half and pre-authorised dropping it. Three reasons it cannot land
cleanly, in order of how fatal they are:

1. It is not a function of the transcript bytes. The project's governing rule
   is that the same session yields the same numbers forever; a `git diff` gives
   a different answer depending on when you run it and what else touched the
   tree since.
2. `render --from facts.json` must work with no repository present. A number
   that only exists when the repo is checked out at the right commit is not a
   fact in this document's sense.
3. The session's start/end commits are not reliably recoverable, and the
   working tree moves for reasons the session did not cause.

Filed as a follow-up rather than forced. If it ever ships it must be a
separately named, clearly *inferred* field — never folded into the exact
counts.

**Making `opaque_edits` smarter about which shell commands actually write.**
Today every `Bash` call counts, so `ls` inflates it. A `sed -i` / `>` / `tee`
predicate would be deterministic and would make the field mean its own name —
but it changes what an existing field counts, which the contract calls
breaking, and it is in neither work item. Follow-up.

## Follow-ups

- Git-diff reconciliation as an explicitly inferred, separately named field.
- Narrow `opaque_edits` to shell calls that plausibly wrote.
- Verify both new code paths against the cleo (Windows) corpus, including the
  `isSidechain` inline shape which kai cannot exercise.
