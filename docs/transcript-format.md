# The session transcript format

What kagviz reads, and what can honestly be recovered from it. Everything here
was established by inspecting real transcripts; nothing is from documentation.
The format is undocumented and drifts between CLI releases, so treat this as a
field guide with a date on it rather than a spec.

_Last checked against CLI 2.1.219 – 2.1.238, August 2026._

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

## Three traps

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

But anything that edits through the **shell** — `sed`, a heredoc, a redirect —
leaves no recoverable diff, and MCP editors carry their own result shapes. A
session that did all its editing through `Bash` shows zero file changes unless
that gap is surfaced. kagviz therefore reports `opaque_edits` beside the line
deltas: a zero that means "nothing changed" and a zero that means "kagviz could
not see it" are different readings, and conflating them makes the whole report
untrustworthy.

This matters more than it sounds. Under some agent instructions, shell editing
is the *default*, so the undercount is systematic rather than occasional.

## What else is recoverable

Already extracted: tool calls by name, failures joined to their call via
`tool_use_id`, `AskUserQuestion` calls with full question text and options,
`Skill` invocations, subagent spawns with `subagent_type` and `resolvedModel`,
pasted images and documents.

Not yet extracted, but present: hook fire counts and errors
(`stop_hook_summary`), API errors, permission-mode and plan-mode transitions,
PR links created during the session, the session's own AI-generated title, and
the `parentUuid` chain that reconstructs turn structure.

Genuinely **not** recoverable from the transcript: usage limit percentages
(statusline-only), and anything the model thought but did not emit.
