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

_Traps 6 and 7 were measured against that same pinned corpus on 2026-08-27
(sprint 013), with the `origin.kind` distribution cross-checked against the
413-session live mirror. No new CLI version: the mirror's newest sessions run
2.1.233 – 2.1.240, inside the range above._

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
| `toolUseResult.persistedOutputPath`, `persistedOutputSize` | A tool result too large for the context is offloaded to `<session-id>/tool-results/<id>.txt`. The `tool_result` block the model saw then holds a `<persisted-output>` placeholder — the path and a 2 KB preview — while `toolUseResult` keeps the full text and these two fields. 11 of ~24,400 results in the kai corpus. The events document's `result_bytes` counts the placeholder, which is what the model was handed; the persisted size is not carried yet. |
| `message.content[].content` (on a `tool_result`) | The result as the model saw it: a string, or an array of `text`/`image`/`tool_reference` blocks. Kept raw; only its text size is read. |
| `isSidechain` | Older format's subagent marker. Newer versions write `subagents/` files instead and leave this `false`. |
| `isMeta` | `true` on `user` records the **harness** wrote, not the user — most visibly the body of an invoked skill. Written as an explicit `false` about as often as it is omitted, so read it as "flagged or not", never as present/absent. `is_user_turn` excludes it. See trap 5. |
| `origin` | `{"kind": …}`, the harness naming what wrote the record. On `user` records only, and with exactly two values corpus-wide: `human` and `task-notification`. The second is flagged by nothing else. See trap 7. |
| `promptSource` | `sdk`, `typed`, `system`, `suggestion_accepted`. **Not** a discriminator — real input and task notifications both read `sdk`. Parsed by nothing; recorded here so the next reader does not reach for it. |

## Seven traps

### 1. `promptId` does not mark a prompt

The obvious reading — "user records with a `promptId` are user prompts" — is
wrong, and wrong in both directions. `promptId` groups **every** record
belonging to one user turn, including tool results and harness-injected text.

Three different things share the `user` channel:

1. **Real prompts** — bare-string `content`, or a block array with `text`,
   `image`, or `document` blocks.
2. **Tool results** — block arrays containing `tool_result`.
3. **Harness injections** — IDE state (`<ide_opened_file>`, `<ide_selection>`),
   local-command output (`<local-command-stdout>`, `<local-command-caveat>`),
   `<system-reminder>`, and attachment placeholders (`[Image: original …]`).

> **Corrected in sprint 005.** This list used to include `<command-name>` and
> its siblings as "slash-command scaffolding", and so did
> `INJECTED_PREFIXES`. That was wrong: `<command-name>` + `<command-args>` is
> the user's own input in structured form, and discarding it threw away the
> prompt. They are off the list, and `command_line` in `src/transcript.rs`
> reads the typed line back out of them. See trap 5 — the record that
> *should* be excluded is the one that follows.

The prefix list is now the **narrow** half of the job. `isMeta` is the
load-bearing half: the harness flags what it wrote, which is more reliable
than matching the shape of it, and is a strict superset of several prefixes
that used to be here. A record is the user speaking when it is unflagged *and*
its content survives the prefix rules.

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

Anything that edits through the **shell** — `sed -i`, a heredoc, a redirect —
leaves no recoverable diff at all. A session that did all its editing through
`Bash` shows zero file changes unless that gap is surfaced. kagviz therefore
reports `opaque_edits` beside the line deltas: a zero that means "nothing
changed" and a zero that means "kagviz could not see it" are different
readings, and conflating them makes the whole report untrustworthy.

This matters more than it sounds. Under some agent instructions, shell editing
is the *default*, so the undercount is systematic rather than occasional.

**But most shell calls are not edits.** Until sprint 013 every `Bash` and
`PowerShell` call was an `opaque_edit`, so the corpus figure of 21,805 "calls
that could have changed files" *was* the shell-call count — and 6,430 of them
are a `grep`, a `sed -n` or a `git status`. `src/shell.rs` now reads the
command string and rules those out, which moves `opaque_edits` to 15,391
without moving a single recovered number.

It is an allow-list, and deliberately so: the error here is one-directional. A
writer judged read-only becomes a zero that should have been an unknown — the
one thing this project promises not to do — while a reader judged a writer
costs only precision. So a command is read-only only when *every* simple
command in it is a known non-writer, and anything unparseable stays opaque:
command substitution, a heredoc, a subshell, a script block, an unterminated
quote.

Two things that audit found, worth carrying:

- **A command-prefix wrapper allow-lists everything behind it.** `env` was on
  the list until a second implementation of the same rule disagreed on one
  call; eight corpus calls turned out to be `env -i … copilot`,
  `env -u … just publish` and `env HOME=… cargo`, all judged read-only. `env`,
  `command`, `sudo`, `timeout`, `nice`, `nohup`, `xargs` and their family are
  now all off it.
- **`2>&1` and `> /dev/null` are not writes.** Reading any `>` as a file write
  left 6,118 read-only calls opaque in an earlier draft — they are the
  overwhelming majority of `>` in the corpus.

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

### 5. A slash command writes two records, and the second one is the harness

Invoking a skill produces **two consecutive `user` records**, parent and child:

```jsonc
// uuid 1f4d9c65 — no isMeta — 158 bytes. This is the user.
{"type":"user","message":{"content":
  "<command-message>start-sprint</command-message>\n
   <command-name>/start-sprint</command-name>\n
   <command-args>korg:1586 procceed with implementation</command-args>"}}

// parentUuid 1f4d9c65 — isMeta: true — 7159 bytes. This is the harness.
{"type":"user","isMeta":true,"message":{"content":
  "Base directory for this skill: /home/ken/.claude/skills/start-sprint\n
   # Start Sprint Skill\n ## User Input\n ```text\n korg:1586 …"}}
```

The first carries what the user typed. The second is the skill definition
being handed to the agent — thousands of characters of instructions that were
never anyone's prompt.

`isMeta: true` is the reliable marker, and it is **not** a synonym for the
prefix list: measured over the 405-transcript corpus, 798 user records carry
it, of which 663 are skill bodies (every skill body has it; none lack it) and
the remaining 135 are `<local-command-caveat>`, `[Image: …]`,
`"Skill /x is already loaded above; instructions unchanged."` and
`"Continue from where you left off."`. It is a strict superset of several
prefixes already on the list.

**kagviz got this exactly backwards until sprint 005** — it discarded record 1
by prefix and counted record 2 as a user turn, so a large share of counted user
prompts were harness text and every real slash-command input was thrown away.
Because phases cut at every user turn, each of those also opened a phase
boundary that should not exist.

Fixed in 005, and measured over the 405-transcript corpus before and after:

| | before | after |
|---|---|---|
| records counted as a user prompt | 2,012 | **1,831** |
| …harness bodies among them | 685 | 0 |
| …real slash-command inputs recovered | 0 | **504** |
| phases | 4,003 | **3,802** |

The two halves are disjoint — no `<command-*>` record in the corpus carries
`isMeta` — so neither fix hides a mistake in the other.

Reading record 1 is a *parse*, not a heuristic: the tags reconstruct
`/start-sprint korg:1586 procceed with implementation` exactly, typo and all.
Read them by name rather than by position — both tag orders occur, every line
after the first may be indented by the emitting command, and `<command-args>`
is sometimes empty (`/clear`) and sometimes absent (`/exit`), which mean the
same thing. `Base directory for this skill:` needs nothing done to it: it is
derivable from the skill name, and the facts already carry a `skills` list.

### 6. One assistant message is written as several records, all with the same usage

**Fixed in sprint 013.** The numbers below are what kagviz reported before it.

The harness writes one assistant API message as **one record per content
block** — `thinking`, `text`, `tool_use` — and stamps every one of them with
the same `message.id` and the *same* `message.usage`:

```
2026-06-16T19:12:08.505Z  assistant  msg_013aTUq198  [thinking]   output_tokens 3088
2026-06-16T19:12:10.849Z  assistant  msg_013aTUq198  [text]       output_tokens 3088
2026-06-16T19:12:10.982Z  assistant  msg_013aTUq198  [tool_use]   output_tokens 3088
```

That is 3,088 output tokens, written down three times. `summary.rs` counts per
record, so it reads 9,264.

Measured over the live mirror's 408 sessions with assistant records:

| quantity | as counted (per record) | actual (per message) | error |
|---|---|---|---|
| `assistant_turns` | 82,416 | 39,343 | **+109.5%** |
| `tokens.output` | 87,992,219 | 33,570,698 | **+162.1%** |

403 of 408 sessions (98%) are affected. Half of all messages are a single
record; the rest run to 35.

The correction needs no judgement: the usage block is **byte-identical across
every record of a message** — 10,604 multi-record messages checked, none
differ — so dedup on `message.id` and take any of them. A record with no
`message.id` counts on its own.

The same family as traps 1 and 5: a field that looks per-turn and is really
per-record. It was found in sprint 012 by reading the app's segment panel
against the transcript behind it — three rows saying `3,088 out` for one
message — and not by any test, because **both** the facts and the events count
it the same wrong way, so every cross-check between them agreed.

**Fixed in 013**, and re-measured over the pinned 405-transcript corpus rather
than the live mirror the discovery was made on:

| | per record | per message | overcount |
|---|---|---|---|
| assistant records / messages | 81,049 | **38,764** | +109.1% |
| `tokens.output` | 83,669,634 | **32,828,298** | +154.9% |
| delegated `assistant_turns` | 1,720 | **603** | +185.2% |
| delegated `tokens.output` | 500,833 | **91,502** | +447.3% |

**391 of 405 sessions** move; the 7 with assistant records that do not are the
ones whose every message is a single record.

Three things checked before the correction was trusted, all over the pinned
corpus:

- **The usage block is byte-identical across every record of a message** — all
  42,285 continuation records, zero differ. So "count it once" needs no choice
  between first, last and max.
- **A message's records are contiguous** — 0 non-contiguous ids. The
  implementation still keys on `message.id` rather than on "same as the last
  one", so an interleaving would dedup rather than double-count.
- **Every assistant record carries `message.id`** — 0 without. The
  count-on-its-own fallback for a record with none is therefore held by unit
  tests, not by a measurement, on the same footing as `isSidechain`.

The correction is in `summary::Counter`, which is the one place a per-record
quantity is read, so the session, every spawn and the events document all take
it together. Two consequences worth knowing:

- A message's tokens land in the bucket and phase where the message
  **opened**, not spread across the records it was written as.
- The events document's promise that a `turn` is followed directly by its
  `tool` events still holds when the calls arrive two records later: the
  records between carry no events of their own. Checked over all 405 sessions
  and every spawn — no turn's `tools` disagrees with the events that follow it.

### 7. A finished background agent writes into the user channel, and nothing flags it

When a spawned agent finishes, the harness writes a `type: user` record whose
content is markup:

```jsonc
{"type":"user","promptSource":"sdk","origin":{"kind":"task-notification"},
 "message":{"content":"<task-notification>\n<task-id>bkumae6rr</task-id>\n
   <tool-use-id>toolu_01Yb…</tool-use-id>\n<status>completed</status>…"}}
```

It carries **no `isMeta`** — absent, not `false` — so trap 5's load-bearing
marker does not catch it, and until sprint 013 `is_user_turn` accepted it. That
cost twice: the harness was counted as the user speaking, and because phases cut
at every user turn, each notification also opened a phase boundary where nobody
had said anything.

**The discriminator is `origin.kind`.** Measured 2026-08-27 over the pinned
405-transcript corpus, every `user` record:

| `isMeta` | `origin.kind` | `promptSource` | shape | n |
|---|---|---|---|---|
| — | — | — | `tool_result` | 44,317 |
| **true** | — | — | skill bodies, placeholders | 797 |
| — | `human` | `sdk` / `typed` / `<none>` / `suggestion_accepted` | real input | 1,064 |
| — | — | — | `<command-*>` scaffold | 448 |
| — | `human` | — | `<command-*>` scaffold | 56 |
| **—** | **`task-notification`** | `sdk` / `system` | **`<task-notification>`** | **115** |
| — | — | `sdk` / `<none>` | text and block prompts | 296 |

`promptSource` is **not** the discriminator — real input and notifications both
read `sdk`. `origin.kind` is, and the match is exact: 115 records carry
`task-notification` and 115 records are `<task-notification>`-shaped, the same
115.

**Two `origin.kind` values exist and no others** — checked over both the pinned
corpus and the 413-session live mirror, across every record type, not just
`user`. `origin` appears on `user` records alone.

So the rule is a **deny-list of one** (`HARNESS_ORIGINS` in
`src/transcript.rs`), not an allow-list of `human`. The direction is the point:
under-counting the user is the same class of lie as over-counting them, so a
kind kagviz has never seen keeps being counted rather than silently vanishing.
A third kind would surface the way this one did — as a prompt that reads like
markup.

Do **not** reach for a `<task-notification>` prefix instead.
`INJECTED_PREFIXES` is the narrow half by design, and `origin` is the harness
*saying* what wrote the record — the same class of evidence as `isMeta`, and
better than matching the shape of the text.

Measured over the corpus, before and after:

| | before | after |
|---|---|---|
| `user_prompts` | 1,831 | **1,716** |
| `phases` | 3,802 | **3,687** |
| sessions affected | | **49 of 405** |
| sessions whose `opened_by` moved | | **0** |

Every notification cut a spurious boundary, which is why the two fall by the
same 115. `opened_by` is untouched on every session — which is exactly why this
never showed on the browse page, and why it took an audit of what a demo would
put on a shared screen to find it.

Found the same way as traps 1, 5 and 6: a field that looks per-turn and is
really per-record, or in this case per-*writer*.

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
