# The session transcript format

What kagviz reads, and what can honestly be recovered from it. Everything here
was established by inspecting real transcripts; nothing is from documentation.
The format is undocumented and drifts between CLI releases, so treat this as a
field guide with a date on it rather than a spec.

_Last checked 2026-08-23 against **405 transcripts** written by CLI 2.1.176 –
2.1.240 (32 distinct versions): 199 on kai and 93 on kubs0 (Linux), 113 on cleo
(Windows). Those three corpora are pinned under `/ai-data/kagviz-data`, with
the facts each produced at a known commit; see its `README.md`. Earlier
revisions of this file cited 2.1.209 – 2.1.238, which was the range checked,
not the range on disk._

## Layout on disk

```
<home>/.claude/projects/
  <project-slug>/                     # cwd with separators flattened
    <session-id>.jsonl                # the session transcript
    <session-id>/
      subagents/agent-*.jsonl         # one file per spawned subagent
      tool-results/*.txt              # large tool outputs, offloaded
```

The transcript is append-only JSON Lines. One session directory can hold
records written by several CLI versions, because a resumed session keeps
appending after an upgrade.

## Record types seen in the wild

`user`, `assistant`, `system`, `attachment`, `queue-operation`, `last-prompt`,
`custom-title`, `ai-title`, `agent-name`, `mode`, `permission-mode`,
`file-history-snapshot`, `file-history-delta`, `pr-link`, `bridge-session`,
`atis-latch`, `frame-link`.

That list is *not* a closed set — it is what turned up in one corpus. New types
appear without warning, which is why `Record` keeps unmodelled fields in `rest`
and never rejects an unknown `type`.

`system` records carry a `subtype`: `stop_hook_summary`, `api_error`,
`turn_duration`, `away_summary`, `local_command`.

## Fields worth knowing

| Field | Notes |
|---|---|
| `timestamp` | RFC 3339, millisecond resolution. On most record types. |
| `sessionId`, `version`, `cwd`, `gitBranch` | Repeated on nearly every record. `version` is the CLI version, and it can change mid-file. |
| `message.usage` | Per-turn tokens: input, output, `output_tokens_details.thinking_tokens`, cache read, cache write. |
| `message.model` | Per-turn model id. A session can span several models. |
| `toolUseResult` | Tool-specific result payload. Shape varies per tool. |
| `isSidechain` | Older format's subagent marker. Newer versions write `subagents/` files instead and leave this `false`. |

## Four traps

### 1. `promptId` does not mark a prompt

The obvious reading — "user records with a `promptId` are user prompts" — is
wrong, and wrong in both directions. `promptId` groups **every** record
belonging to one user turn, including tool results and harness-injected text.

Three different things share the `user` channel:

1. **Real prompts** — bare-string `content`, or a block array with `text`,
   `image`, or `document` blocks.
2. **Tool results** — block arrays containing `tool_result`.
3. **Harness injections** — IDE state (`<ide_opened_file>`, `<ide_selection>`),
   slash-command scaffolding (`<command-name>`, `<local-command-stdout>`,
   `<local-command-caveat>`), `<system-reminder>`, and attachment placeholders
   (`[Image: original …]`).

Worse, injections arrive as **sibling blocks in the same record as real user
text** — an `<ide_opened_file>` block followed by what the user actually typed.
So a record cannot be classified by its first block; every block has to be
checked. See `INJECTED_PREFIXES` in `src/transcript.rs`.

Measured on one 2.5-hour session: the naive `promptId` rule reported 14 user
prompts, of which **zero** were real — 3 were slash-command scaffolding and 11
were image placeholders. The correct rule reports 26.

### 2. Wall-clock span is not session length

A resumed session can span days while holding under an hour of work. One
observed transcript spans 6 days 20 hours and contains 52 minutes of activity.
kagviz reports active time (gaps of 2 minutes or more excluded) alongside wall
and idle, because any one of the three alone misleads.

### 3. File changes are only partly visible

`Edit`, `Write` and `NotebookEdit` results carry a `structuredPatch`: real
unified-diff hunks, so per-file line deltas are exact. A `create` result has an
empty patch and the whole file body in `content`, so its line count is the
addition.

Anything that edits through the **shell** — `sed`, a heredoc, a redirect —
leaves no recoverable diff at all. A session that did all its editing through
`Bash` shows zero file changes unless that gap is surfaced. kagviz therefore
reports `opaque_edits` beside the line deltas: a zero that means "nothing
changed" and a zero that means "kagviz could not see it" are different
readings, and conflating them makes the whole report untrustworthy.

This matters more than it sounds. Under some agent instructions, shell editing
is the *default*, so the undercount is systematic rather than occasional.

**MCP file servers carry their own diff, and it is recoverable.** Sprint 003
added an adapter table for them. The measured shape (`mcp__kaed-*__edit`) is
the trap: `toolUseResult` is a **JSON string**, not an object.

```json
"toolUseResult": "{\"applied\":true,\"diff\":\"--- a/m.yml\\n+++ b/m.yml\\n@@ -1,3 +1,4 @@\\n ctx\\n-gone\\n+one\\n\",\"files\":[{\"path\":\"m.yml\"}],\"txn_id\":28}"
```

Parse the string, then read `diff` as a unified diff and `files[].path` for the
files. `"applied": false` means the server refused the edit — a *known* zero,
not an unknown, so it must not be counted as opaque. Note the paths are
root-relative and may not even name a file on this host.

**The two halves arrive independently.** 16 results in the cleo corpus are
`{"applied":true,"files":[…]}` with **no `diff` key** — the edit landed and
named its files exactly, and only the line counts are absent. Treating that as
"unreadable" drops file paths the transcript is holding (35 of them, corpus
-wide). Take the files, and still charge `opaque_edits` for the missing lines.
No transcript on kai or kubs0 has this shape; it was found only because a
Windows corpus was snapshotted.

Counting a unified diff by `+`/`-` prefix is wrong: a deleted line whose own
content is `--` arrives as `---` and gets eaten as a file header. Count only
lines *inside* a hunk (after `@@`), and match the header as the `--- `/`+++ `
pair. Markdown is full of `---`.

**`file-history-delta` is a dead end. Do not spend the afternoon.** The record
type looks exactly like what the shell-edit gap needs — the harness tracking
every file it saw change, with `trackingPath` (relative to `cwd`, or to
`backup.realParentDir` when the file is outside it) and a backup reference —
and there are 1,610 of them in the corpus. Measured in sprint 003: the set of
files it names is a strict **subset** of the files `structuredPatch` already
covers. Tracked-but-not-patched, corpus-wide: **0**. It is a
backup-before-`Edit` marker, not a filesystem watcher, and it cannot see a
shell edit.

### 4. `null` is not the same as absent

At least one CLI version writes

```json
"usage": { "output_tokens": 118, "output_tokens_details": null }
```

Serde's `#[serde(default)]` covers an **absent** field only. A field that is
present and `null` is still handed to that field's own deserializer, which
rejects it — and the failure is not scoped to the field. The whole record is
rejected, so the line is skipped and that turn's **tool calls, timestamp and
model** vanish along with its token counts. One such line in a 4,000-line
transcript is a silent hole in every number downstream, and nothing about the
output looks wrong.

The fix is a `null_as_default` deserializer (`Option::<T>::deserialize` then
`unwrap_or_default`) on every non-`Option` field that carries a `default`. See
`src/transcript.rs`. Assume the next drift of this kind is already on disk:
when adding a typed field, make it either `Option` or `null`-tolerant, never
merely defaulted.

Frequency, measured: exactly **1** occurrence across 305 transcripts, in one
line of one session. It cost nothing to find only because the corpus sweep
asserts *zero* skipped lines rather than "few".

## Line endings

Transcripts are written **LF even on Windows** — checked across 40 cleo
transcripts, none CRLF. Do not rely on that: `read()` trims each line, so a
stray `\r` is absorbed either way.

## Subagents

`subagents/agent-<agentId>.jsonl`, one file per spawn, holding ordinary
records. They are self-identifying twice over: every record carries `agentId`,
and the same id is in the file name. They also carry the **parent's**
`sessionId`, so a sidecar is never ambiguous about which session it belongs to.

The join back to the parent is the `Agent` tool result, whose `toolUseResult`
is an object:

```json
{ "agentId": "ad59e1be0a55a2ed0", "description": "Summarize sprints 013-020 deltas",
  "resolvedModel": "claude-opus-4-8[1m]", "isAsync": true, "status": "async_launched",
  "outputFile": "…", "canReadOutputFile": true, "prompt": "…" }
```

`subagent_type` is on the *call*'s `input`, not the result, so both halves are
needed. `outputFile` is not: the file name carries the id already.

The undercount here is severe. One corpus spawn ran **48 tool calls and 25k
output tokens** while its parent recorded a single `Agent` call.

**The drift:** older CLI versions inlined subagent turns into the main
transcript with `isSidechain: true` instead of writing sidecars. Both shapes
must be read. Note the direction of the error is *opposite* — inlined records
inflate the parent rather than hiding the subagent — so they have to be lifted
out of the parent's counts, not just noticed. There are **zero** such records
in the kai corpus, so that path is held by unit tests, not by a measurement.

## What else is recoverable

Already extracted: tool calls by name, failures joined to their call via
`tool_use_id`, `AskUserQuestion` calls with full question text and options,
`Skill` invocations, subagent transcripts folded in as a separate tier (see
**Subagents** above), MCP file-server diffs, pasted images and documents.

**`AskUserQuestion` answers.** The question text and options are in the
`tool_use` block's `input.questions[]`. What the user *chose* is in the
`toolUseResult` of the matching result record, under `answers` — an object
keyed by the **question text itself**, valued with the chosen option's `label`:

```json
"toolUseResult": {
  "questions": [ … ],
  "answers": { "Which store?": "Postgres" },
  "annotations": { … }
}
```

Join on `tool_use_id`, then on the question string. The `tool_result` block's
`content` prose says the same thing, but it is a formatted sentence with the
answers interpolated — parse `answers`, not the sentence. A question with no
matching key was never answered (an interrupted prompt); record that as unknown
rather than assuming the first option.

Not yet extracted, but present: hook fire counts and errors
(`stop_hook_summary`), API errors, permission-mode and plan-mode transitions,
PR links created during the session, the session's own AI-generated title, and
the `parentUuid` chain that reconstructs turn structure.

Genuinely **not** recoverable from the transcript: usage limit percentages
(statusline-only), and anything the model thought but did not emit.
