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

Every file-change **quantity** is in exactly one of two states:

1. **Recovered** — an exact number, read out of what the transcript actually
   carries. Contributes to `lines_added` / `lines_deleted` / `files_touched`.
2. **Unrecovered** — could have changed and exposed nothing readable. Counted
   in `opaque_edits`, never folded into the deltas.

There is no third state in the shipped facts. **Inferred numbers are not
shipped** — see "What we rejected".

The word doing the work there is *quantity*, not *call*, and the distinction is
not pedantic: one call can be recovered in one quantity and unrecovered in
another. We got this wrong on the first pass and the corpus caught it — see
"The defect only cleo could find".

`changes.by_tool` is the audit surface that makes this checkable rather than
believed: per tool, how many edit-capable calls there were, what was recovered
from them, and how many stayed opaque. It is the same argument `mix` makes for
a phase's `kind` — carry the counts the label came from.

The subagent tier is the same rule applied to work rather than to lines:
delegated work is reported as its **own tier** with an explicit combined line,
never silently merged into the parent's totals. Burying delegated cost inside
the parent hides the number a reader most wants to see.

## What the corpus taught us

Swept 199 transcripts on kai first, then — once the extractor was written —
pinned and swept **all three hosts**: 405 transcripts, 568 MB, zero parse
failures, zero skipped lines. See "The pinned corpora" below; the second sweep
found a defect the first could not have.

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

**No `isSidechain: true` records on any host.** All three run CLI 2.1.176+,
which is past the inline format. That path is written from the format's shape
and held by unit tests, not by a measurement — now for a stated reason rather
than for lack of looking.

The thin-corpus warning that was true of kai alone — 3 `Agent` calls across 2
sessions — is why the other two hosts got pinned. kubs0 has **18** spawns
across 9 sessions, and cleo has **152** kaed edit calls against kai's 7.

### The defect only cleo could find

16 kaed results on cleo are `{"applied":true,"files":[…]}` with **no `diff`
key**: the edit landed and named its files exactly, and only the line counts
are absent. The first implementation marked the whole call opaque and threw the
file list away — under-reporting `files_touched` by 35 paths.

The framing was right and the code did not follow it. The contract says *every
file-change **quantity*** is recovered or unrecovered; the implementation
decided that per **call**. They are not the same, and this shape is where they
come apart: files recovered, lines not. `Recovered` now carries `lines_known`,
so a result contributes its files and still charges `opaque_edits` for the
lines it did not carry. `opaque` in `by_tool` therefore means "line counts
unreadable", not "call unreadable" — a tool can show non-zero `opaque` beside
an exact `files_touched`.

Effect, measured across all three corpora: kai and kubs0 unchanged (neither has
the shape), cleo `files_touched` 597 → 632 over 4 sessions, `opaque_edits`
unchanged at 3,253 — those calls' lines were unknown before and still are.

## The pinned corpora

The live corpus prunes itself on roughly a 30-day window; a session vanished
from under the first sweep *while it was running*. Several result shapes exist
in one or two transcripts, so without a pinned copy the only real-world
validation of an extractor path silently stops existing and nothing fails.

`/ai-data/kagviz-data` now holds a verbatim snapshot per host plus the facts
each produced at a known commit — inputs and outputs, because the corpus proves
the extractor still parses and the baseline proves no number moved.

| corpus | transcripts | spawns | kaed edits | size | CLI range |
|---|---|---|---|---|---|
| kai | 199 | 3 | 7 | 318 MB | 2.1.176–2.1.240 |
| kubs0 | 93 | 18 | 14 | 133 MB | 2.1.201–2.1.240 |
| cleo (Windows) | 113 | 10 | 152 | 117 MB | 2.1.209–2.1.238 |

cleo carries the load for this sprint's work: three kaed server-name variants
including `mcp__kaed__edit` with no host suffix, the only `PowerShell` traffic
anywhere (152 calls), and the no-`diff` shape above. kubsdb has no
`~/.claude/projects` and is not a source.

Two other things now settled by measurement: the CLI range on disk is
2.1.176–2.1.240, wider than `docs/transcript-format.md` claimed; and
LF-even-on-Windows holds (20 cleo transcripts sampled, zero CRLF).

The corpora are raw session content and stay on that volume. A repo fixture is
wanted eventually — so someone else can reproduce these numbers exactly — but
it must be a hand-minimised, reviewed extract, not a file copied off the
volume. Not needed yet.

## What shipped

Facts, additively except where noted:

- `changes.by_tool` — per-tool `{calls, files_touched, lines_added,
  lines_deleted, opaque}`.
- `delegation` — `{spawns[], unjoined_spawns, inline_records, totals}`.
- **Moved on purpose:** `changes.*` on the 3 kai transcripts carrying kaed
  edits, which were previously invisible in both directions; and `files_touched`
  on 4 cleo sessions, from the no-`diff` fix.

Outside the repo:

- **The first pinned regression corpora** — kai, kubs0 and cleo under
  `/ai-data/kagviz-data`, 405 transcripts and 568 MB, each with the facts it
  produced at both sprint-003 commits. The corpus proves the extractor still
  parses; the baseline proves no number moved. `README.md` there carries the
  sweep as a runnable block.

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
- Narrow `opaque_edits` to shell calls that plausibly wrote. Now the largest
  remaining undercount by a wide margin: 21,821 opaque calls across the three
  corpora, most of which never touched a file.
- Surface that `files_touched` is *also* a floor wherever shell calls ran. The
  facts say so now; the report does not.
- A hand-minimised, secret-cleaned fixture in the repo, so the corpus sweep is
  reproducible by someone without access to `/ai-data`.
