# The facts document

`kagviz show <id> --json` emits one JSON object: everything kagviz was able to
count about a session. It is the **only** input the renderer takes, and it is
the seam the front-end plugs into — typed in `web/src/lib/contract/` since
sprint 011, with a conformance test over `tests/golden/` that runs inside
`just check`, so a change here that breaks a consumer fails the build on this
side of the seam. Two more documents live
under the same rules and are described at the end: `sessions.json`, the index
a consumer reads *first*, and the [events document](#the-events-document),
the detail tier it reads *last*.

Treat it as a contract:

- **Adding a field is not a breaking change.** Consumers must ignore fields
  they do not know.
- **Changing or removing a field is.** So is changing what an existing field
  counts.
- **Every value is computed from the transcript bytes** — with exactly one
  named exception, `labels`, which is the only place a model has ever written
  into this document, is absent unless asked for, and never replaces a value.
  Everything outside `labels` was counted. See [`labels`](#labels--the-only-model-written-field).
- **An unknown is never rendered as a zero.** Where kagviz cannot see
  something, the document says so with a separate field (see `opaque_edits`)
  rather than reporting a confident number it does not have.
- **An optional field is absent, never `null`.** A resumed phase has no
  `opened_by` key; an unanswered question has no `chosen`; a spawn that could
  not be joined has no `subagent_type`. A consumer must treat a missing key
  and a `null` the same way — and since 009 it only ever sees the first.

### Breaking changes, in order

A breaking change is allowed; making one silently is not. Each one lands with
the numbers it moved, measured over the pinned corpus, so a consumer can tell
whether it is affected without re-deriving anything.

| Sprint | What changed | Measured over 405 transcripts |
|---|---|---|
| 005 | `user_prompts`, `phases`, `user_involvement` and `activity…buckets[].user_turns` stop counting harness-written records (`isMeta`) and start counting slash-command invocations. | `user_prompts` 2,012 → **1,831**: 685 harness bodies dropped, 504 real user inputs recovered. `phases` 4,003 → **3,802**. |
| 005 | `active_secs` is redefined as the sum of the span lengths — see below. | 560,283s → **558,730s**; 305 of 405 sessions corrected, every one downward, worst −198s. |
| 009 | Optional fields are **absent** instead of `null` — every `Option` the document carries: `opened_by`, `chosen`, `header`, `at`, a spawn's `agent_id`/`subagent_type`/`description`/`model`/`started`/`ended`, the top-level `session_id`/`project`/`cwd`/`git_branch`/`started`/`ended`. The contract had promised this since it was written; the serializer only kept it for `labels`. | Bytes change on **397 of 405** sessions; **no value moves**. What had been `null`: `opened_by` 1,971 times (resumed phases), `chosen` 6, `subagent_type` and `description` 5 each (unjoined spawns), `cwd`/`git_branch`/`started`/`ended` once each (one session with no timestamped record). |
| 009 | `subagents` is the sorted **set** of subagent types invoked, one entry each — as `skills` already was. How many times is `tool_calls` and `delegation`'s job. | 8 of 405 sessions: 21 entries → 9 (`Explore,Explore` → `Explore`). |
| 013 | `changes.opaque_edits` and `by_tool.<shell>.opaque` count only the shell calls whose **command string** could not be ruled out. A call is read-only when every simple command in it is a known non-writer and nothing in it can redirect to a file, substitute, or interpret; anything the tokenizer cannot split stays opaque. `by_tool.<shell>.calls` is unchanged — it is still every call, so the read-only share is the difference. | `opaque_edits` 21,821 → **15,391**; shell `opaque` 21,805 → **15,375** of the same 21,805 `calls` (29.5% judged read-only). **296 of 405** sessions. `files_touched`, `lines_added` and `lines_deleted` do not move — nothing became *recovered*, only known-empty. |
| 013 | `assistant_turns`, `models`, every field of `tokens`, `phases[].output_tokens`, `activity…buckets[].output_tokens` and the delegated tier's equivalents count one **message** rather than one record. The harness writes one API message as one record per content block, all stamped with the same `usage`; see `transcript-format.md` trap 6. | `assistant_turns` 81,049 → **38,764** (+109.1% as counted), `tokens.output` 83,669,634 → **32,828,298** (+154.9%). Delegated: turns 1,720 → **603**, output 500,833 → **91,502**. **391 of 405** sessions. A message's tokens now land in the bucket and phase where it **opened**. |
| 013 | `user_prompts`, `phases`, `user_involvement` and `activity…buckets[].user_turns` stop counting `<task-notification>` records — the harness reporting a finished background agent into the user channel, flagged by no `isMeta`. The discriminator is `origin.kind`; see `transcript-format.md` trap 7. | `user_prompts` 1,831 → **1,716** and `phases` 3,802 → **3,687**, both −115: every notification also cut a phase boundary. **49 of 405** sessions. `opened_by` moves on **0** of them — no session was opened by one, which is why the browse page never showed this. |

Each row names what it did *not* touch as precisely as what it did. 005 left
`wall_secs`, `idle_secs`, `tokens`, `tool_calls`, `tool_failures`, `changes`
and the span boundaries byte-identical. 009 moved no value at all: strip the
`null`s from the a8dad05 baseline and dedup its `subagents`, and every one of
the 405 documents is byte-identical to the new output.

The three 013 rows are one sprint and one baseline regeneration, but three
independent corrections, so each is stated against the **same** pre-change
baseline (`19a75d4`, which `main` still reproduced byte for byte on all 405
transcripts when the sprint opened) rather than against the row above it. They
barely overlap: the `<task-notification>` row moves the user-side four and
nothing else; the `message.id` row moves the assistant-side counts and nothing
else; the `opaque_edits` row moves `changes` and nothing else. Where a phase
merged, its own `output_tokens`, `records` and `mix` are the sum of the two it
replaced — the session totals those roll up to did not move.

The renderer round-trips it: a report built from a serialized facts document is
byte-identical to one built from the summary in memory, and byte-identical
across Linux and Windows. There is a test that says so
(`render::tests::a_report_from_serialized_facts_is_identical`), and it exists
to catch this seam quietly rotting.

## Shape

Identity and environment:

| Field | Meaning |
|---|---|
| `session_id`, `project`, `cwd`, `git_branch` | As recorded on the transcript's records. |
| `cli_versions` | Every CLI version that wrote into this file — a resumed session spans upgrades. |
| `models` | model id → assistant turns on that model. Counts what `assistant_turns` counts, so the values sum to it — one **message** each, not one record. |

Time — all three are reported because any one alone misleads:

| Field | Meaning |
|---|---|
| `started`, `ended` | First and last timestamped record, RFC 3339 UTC. |
| `wall_secs` | `ended - started`. |
| `idle_secs` | Sum of inter-record gaps of `IDLE_GAP_SECS` (120s) or more. |
| `active_secs` | The sum of the `activity.spans` lengths — the stretches of work, each measured once. **Not** `wall_secs - idle_secs`: that is the same quantity through two truncations against the spans' one each, and on a session with many spans the two drift (198s of 12h39m at 209 spans). Read off the spans, so `active_secs`, the strip and the phase durations are one number rather than three that nearly agree. Changed in 005. |

Volume and outcome:

| Field | Meaning |
|---|---|
| `records` | Transcript lines parsed. |
| `skipped_lines` | Lines that did **not** parse. Non-zero means every number here is partial, and the report says so on the page. |
| `assistant_turns`, `user_prompts` | **A turn is a message, not a record**, on both sides, and neither is the same as `records`. Four traps, all in `transcript-format.md`: `promptId` rides on injected records too (1); a record the harness wrote carries `isMeta` — including the instruction document a skill hands the agent, though the `<command-*>` scaffold beside it **is** the user (5); one assistant message is written as several records sharing one `usage`, so `assistant_turns` counts distinct `message.id` (6, corrected in 013); and a `<task-notification>` is the harness reporting a finished background agent, flagged only by `origin.kind` (7, corrected in 013). |
| `tool_calls`, `tool_failures` | tool name → count. Failures are joined to their call by `tool_use_id`; a failure whose call is not in the file is blamed on `<unknown>` rather than dropped. |
| `tokens` | input / output / thinking / cache read / cache write. |
| `changes` | `files_touched`, `lines_added`, `lines_deleted` recovered from a diff, plus `opaque_edits` and a per-tool `by_tool` breakdown. |
| `skills`, `subagents`, `subagent_transcripts` | What the session delegated to: the sorted set of skill names and of subagent types invoked, one entry each (`subagents` since 009). The delegated *work* — and how many spawns there were — is in `delegation`. |
| `labels` | **Absent unless asked for.** The one model-written field — see below. |

### `changes` — exact, or visibly absent

Every file-change quantity is in exactly one of **two** states, and the
document says which:

- **Recovered** — an exact number, read out of a diff the transcript actually
  carries. It is in `lines_added` / `lines_deleted` / `files_touched`.
- **Unrecovered** — a call that could have changed a file and exposed nothing
  readable. It is in `opaque_edits`, and it is never folded into the deltas.

There is no third state. An **inferred** number — a `git diff` over the
session window being the obvious candidate — is not a function of the
transcript bytes, and does not go in this document. If one ever ships it will
be a new, separately named field that says *inferred* on it, and it will not
touch the counts above. See `sprints/003-close-the-undercount.md` for why the
reconciliation was rejected rather than attempted.

#### `changes.opaque_edits`

Calls that could have changed files and left no recoverable diff. `lines_added`
is a floor, not a total, and under agent instructions that prefer shell editing
the shortfall is systematic rather than occasional.

Read it precisely: it counts calls whose **line deltas** are unknown, which is
not the same as calls kagviz learned nothing from. Four things land here:

- A `Bash` / `PowerShell` call whose **command string could have written**,
  counted from the call rather than from a result — an interrupted shell call
  leaves no result and is still an edit kagviz cannot see.

  Since 013 that is not every shell call. `src/shell.rs` reads the command and
  a call is read-only only when *every* simple command in it is a known
  non-writer and nothing in it redirects to a file, substitutes a command,
  opens a subshell or script block, or feeds an interpreter — and anything the
  tokenizer cannot split stays opaque. It is an **allow-list**, because the
  error that matters here is one-directional: a writer judged read-only becomes
  a zero that should have been an unknown, which is the one thing this document
  promises never to do. A reader judged a writer costs only precision.

  `by_tool.<shell>.calls` stays the **total**, so the share judged read-only is
  visible as `calls - opaque` rather than taken on faith. That is the audit
  surface for the allow-list, the same argument `by_tool` itself makes.
- A file-editing MCP tool whose result no adapter could read.
- A built-in editor (`Edit`, `Write`, `NotebookEdit`) whose result kagviz could
  not read **and which did not error**. A *failed* edit changed nothing and is
  a known zero; it is already visible in `tool_failures`.
- A result that named its files but carried **no diff** — measured shape,
  `{"applied":true,"files":[…]}` from a file server. Its files are exact and
  are counted in `files_touched`; only its lines are missing. So a call can be
  opaque here *and* have contributed to `files_touched`, and the two must be
  read separately rather than as one verdict on the call.

`files_touched` has its own floor, and it is not the same one. A `Bash` call
that wrote a file leaves no path to count, so `files_touched` is also a lower
bound wherever `opaque_edits` includes shell calls.

#### `changes.by_tool`

```json
"by_tool": {
  "Bash":                { "calls": 20, "files_touched": 0, "lines_added": 0,   "lines_deleted": 0, "opaque": 20 },
  "Edit":                { "calls": 3,  "files_touched": 2, "lines_added": 18,  "lines_deleted": 6, "opaque": 0 },
  "mcp__kaed-kai__edit": { "calls": 7,  "files_touched": 3, "lines_added": 25,  "lines_deleted": 1, "opaque": 0 }
}
```

The audit surface for the adapter table — the same argument `mix` makes for a
phase's `kind`. Without it, "+340 −88, and 51 unseen" has to be taken on faith.

`calls` is edit-capable calls of that tool; `opaque` is how many of them gave
no readable **line counts**. Summing `by_tool` gives back `lines_added`,
`lines_deleted` and `opaque_edits` exactly — there is a test.

**Do not read `opaque == calls` as "this tool recovered nothing".** It was a
safe proxy until 013, and is not one now: a shell tool can have calls that are
neither readable nor opaque, because their command provably wrote nothing. The
three renderers were all switching on that equality, and all three now assemble
the line from what is known — `files_touched`/`lines_added`/`lines_deleted`
first, the opaque count second, and "nothing written" when there is neither.

A tool can show a non-zero `opaque` *and* an exact `files_touched`: that is the
files-without-a-diff case above, and it is the reason `opaque` is not simply
"calls we could not read".

`files_touched` does **not** sum, because it must not: two tools that edited
the same file changed one file between them. Note also that a file is
identified by whatever path the tool reported, and an MCP file server usually
reports a root-relative path — possibly on another host. `files_touched`
counts distinct identifiers, not resolved filesystem paths.

### `delegation` — work the session handed to subagents

Added in sprint 003, additively: nothing above moved.

```json
"delegation": {
  "spawns": [
    { "agent_id": "a3f518e6…", "subagent_type": "Explore", "model": "claude-opus-4-8[1m]",
      "description": "Map linking-layer code", "sidecar": true,
      "started": "…", "ended": "…", "active_secs": 323,
      "records": 143, "skipped_lines": 0, "assistant_turns": 92,
      "tool_calls": { "Bash": 31, "Read": 17 }, "tool_failures": {},
      "tokens": { … }, "changes": { … } }
  ],
  "unjoined_spawns": 0,
  "inline_records": 0,
  "totals": { "records": 166, "assistant_turns": 105, "tool_calls": { … },
              "tool_failures": { … }, "tokens": { … }, "changes": { … } }
}
```

**A tier, not an addend.** The session's own numbers are exactly what they
were before this field existed: a session that spawned two agents still made
two `Agent` calls, and that is what `tool_calls` says. Delegated cost stands
beside it. Burying it inside the parent would hide the number a reader is most
often here for — one corpus spawn ran 48 tool calls and 25k output tokens
behind a single `Agent` call.

Leaving the reader to add the two up is the same failure in miniature, so the
sum is stated explicitly. It is a *method* rather than a field —
`combined_tool_calls`, `combined_tool_failures`, `combined_output_tokens`,
following `total_tool_calls` — because the facts carry each tier once and a sum
anyone can recompute is not a separate fact. What is not optional is showing it.

`tool_failure_rate` is a method for the same reason: a quotient of two fields
already here. Its denominator is `tool_calls` — a failed call is a call, counted
once — and failures blamed on `<unknown>` are left out of the numerator, since
their calls are not in the denominator either. A consumer computing the rate
itself should make the same two choices, or its number will disagree with the
report's.

**`active_secs` does not combine.** A subagent runs while the session waits on
it, so those seconds overlap rather than add. There is deliberately no
`combined_active_secs`; tokens add across concurrent agents, seconds do not.

A spawn's `active_secs` carries the same definition as the session's — the
stretches of its own work, summed — so the two are comparable side by side.
The delegated tier has no `activity`, so there are no spans to read it off;
the definition is applied to the spawn's timestamps directly.

**Subagents are absent from `activity` and `phases`.** Those cut the *parent's*
timeline, and a concurrent agent has no position on it.

| Field | Meaning |
|---|---|
| `spawns[]` | One per delegated agent, ordered by start time. `sidecar` is `true` when the numbers came from a `subagents/agent-*.jsonl` file, `false` when they came from `isSidechain` records an older CLI inlined into the parent. `subagent_type`, `description` and `model` are absent when the spawn could not be joined to an `Agent` call. |
| `unjoined_spawns` | `Agent` calls with no transcript to read. The work happened and kagviz cannot see it — an unknown, not a zero. |
| `inline_records` | Records lifted *out* of the parent's counts because they were inlined subagent turns. Reported so the move is visible rather than silent. |
| `totals` | The tier summed. `changes.files_touched` merges the path sets across spawns rather than adding the counts. |

### `labels` — the only model-written field

Added in sprint 004, additively, and **absent** from any document that did not
ask for it: `kagviz show --json` emits no `labels` key at all unless `--label`
was passed. Verified over the 405-transcript corpus — every session's facts are
byte-identical to the sprint-003 baseline without the flag.

```json
"labels": {
  "headline": "Closed the file-change undercount with an adapter table.",
  "phases": [ { "phase": 0, "label": "reading the extractor" },
              { "phase": 3, "label": "chasing the Windows diff" } ],
  "model": "qwen2.5-7b-instruct",
  "prompt_version": "headline.v1",
  "facts_digest": "sha256:74f72c649f78aab10…",
  "generated": "2026-08-23T22:14:07Z"
}
```

**Read the boundary literally.** Everything inside this object was written by a
model; everything outside it was counted. A consumer that ignores the whole key
gets exactly the document kagviz emitted before sprint 004 — that is what
"additive" means here, and it is the reason the phase labels are a **parallel
array keyed by phase index** rather than a `label` field on `Phase`. A written
label sitting one line below `tool_calls`, in the same object, with only a field
name to tell them apart, is the confusion the whole project is built to avoid.

| Field | Meaning |
|---|---|
| `headline` | One sentence over the session. At most 160 characters; longer replies are cut, not rendered whole. |
| `phases[]` | `phase` is an index into the facts' `phases` array; `label` is at most 60 characters. **Sparse** — a phase with no entry has no label, and that is an *absent* label, never a blank one. |
| `model`, `prompt_version` | Who wrote it and which prompt did. The prompt is versioned in the repo under `prompts/`. |
| `facts_digest` | `sha256` over this document with `labels` removed. Recompute it to learn whether the prose still describes the counts. |
| `generated` | When the model was asked. Comes from the cache on a hit, so re-rendering does not change bytes just because time passed. |

#### No number is ever model-produced

Not by policy — by construction. The digest of facts handed to the model
carries **no quantities at all**: no counts, no durations, no token totals, not
even the number of phases. It carries ranked tool *names*, ordinal phase sizes
(`long`/`medium`/`brief`), and the user's own words. A model that is never shown
a measurement cannot echo one into a sentence that then disagrees with the panel
below it, and the first time those disagreed the whole report would stop being
worth reading.

Audited over all 405 corpus transcripts: every digit reaching the model is
either inside the user's own quoted words or part of a recorded name
(`mcp__kaed-kubs0__read`, `deploy-kubs0`, `git branch: 003-curator`). Zero
computed quantities.

#### Reproducibility

Labels are cached on `facts_digest` (plus the prompt's own bytes), so:

- **Facts identical → the same labels forever**, with no model involved. A
  cache hit never contacts the backend, so a labelled report re-renders
  byte-for-byte with the model host switched off.
- **Facts changed → the old labels are not reused.** They were written about a
  different session. This means an additive change to the facts — like this one
  — invalidates every cached label, which is correct rather than unfortunate.

The cache lives in `<transcript-root>/.kagviz/labels/`, overridable with
`--label-cache`. A model call is not reproducible on its own (`temperature: 0`
and a fixed seed are the most a served model offers, and batching alone can move
a token); the cache is what makes the *report* reproducible, which is why it is
a mechanism here rather than an optimization.

#### When the backend is unreachable

The report renders **without a headline**, and stderr says why. Failing the
render would make a model a dependency of the deterministic page — the exact
inversion this field is fenced off to prevent. Absent headline, not empty
headline; the layout is built around having none, because that is the default
path.

### `sessions.json` — the cross-host index

Added in sprint 007. **A second contract, not part of the facts document**: it
is written by `kagviz derive` (and `kagviz index`) into `derived/`, one row per
session across every mirrored host, and it is the file a browse page or a
front-end reads *first* — to choose a session before fetching its facts.
The same rules apply: adding a field is not a breaking change, changing or
removing one is, and every figure is copied or summed from that session's
facts document. Nothing in it is computed from a transcript directly, and
nothing in it is inferred.

```json
{ "sessions": [
  { "host": "kai", "session_id": "63a9b83b-…",
    "project": "-home-ken-5090", "cwd": "/home/ken/5090", "git_branch": "main",
    "started": "…", "ended": "…", "wall_secs": 9000, "active_secs": 2280,
    "user_prompts": 7, "assistant_turns": 41,
    "tool_calls": 83, "tool_failures": 2,
    "files_touched": 3, "lines_added": 57, "lines_deleted": 3, "opaque_edits": 28,
    "output_tokens": 35663, "phases": 44, "delegated_spawns": 2, "skipped_lines": 0,
    "models": ["claude-opus-5"], "cli_versions": ["2.1.240"],
    "opened_by": "fix the failing test",
    "headline": "Closed the file-change undercount with an adapter table.",
    "facts": "facts/kai/63a9b83b-….json", "report": "reports/kai/63a9b83b-….html",
    "source_digest": "sha256:…", "kagviz": "0.1.0 (a1b2c3d)" }
] }
```

An object holding an array rather than a bare array, so a top-level field can
be added later without breaking a consumer.

| Field | Meaning |
|---|---|
| `host` | The mirror the session came from — the directory name under `live/`. Not in the facts, which are host-agnostic. |
| `session_id` | The transcript's file stem, which is what `kagviz show <id>` takes. |
| `project`, `cwd`, `git_branch`, `started`, `ended`, `wall_secs`, `active_secs`, `user_prompts`, `assistant_turns`, `skipped_lines`, `models`, `cli_versions` | Copied from the facts. `models` and `cli_versions` are the facts' keys and set, as sorted arrays. |
| `tool_calls`, `tool_failures` | The facts' per-tool maps summed — the session's own tier, exactly `total_tool_calls()`. Delegated work is **not** folded in; `delegated_spawns` says how many agents there were. |
| `files_touched`, `lines_added`, `lines_deleted`, `opaque_edits` | The facts' `changes` totals. Read `opaque_edits` the way the facts say to: the deltas are a floor wherever it is non-zero. |
| `output_tokens` | `tokens.output`. |
| `phases` | How many. |
| `opened_by` | The first non-empty prompt preview in `user_involvement` — what the session was opened with. **Absent** when there is none. |
| `headline` | `labels.headline`, when the facts carry labels. Written by a model, not counted — the same boundary the facts draw, and the index page marks it the same way. **Absent** otherwise. |
| `facts`, `report`, `events` | Paths relative to the derived root, which is the served root. `events` (added in 009) is the [events document](#the-events-document) for the session. |
| `source_digest`, `kagviz` | From `state.json`: the sha256 over the transcript bytes the facts were derived from, and the kagviz version that derived them. Absent only for a facts file the derive did not write (dropped in by hand). |

**Optional fields are absent, never `null`.** This contract kept from the
start what the facts document had promised and, until 009, did not fully
deliver — the `null`-vs-absent drift review 006 found was closed there, and
the two documents now follow one rule.

Ordering is by `started`, newest first, then host, then id — stable across
runs, so the file is byte-identical when nothing was derived.

The index page (`index.html`) beside it is rendered from this file alone, the
same way the report is rendered from the facts alone.

### `activity` — the time series

Added in sprint 001, additively: the totals above did not move.

```json
"activity": {
  "bucket_secs": 30,
  "spans": [
    { "started": "…", "ended": "…", "secs": 900, "idle_before_secs": 0,
      "buckets": [ { "records": 4, "tool_calls": 3, "tool_failures": 0,
                     "user_turns": 1, "output_tokens": 812 } ] }
  ]
}
```

A **span** is a stretch of continuous work; spans are cut wherever a gap
reaches `IDLE_GAP_SECS`, and `idle_before_secs` carries the gap that precedes
each one (`0` for the first). Idle occupies no buckets at all — that is what
lets a renderer collapse it.

`bucket_secs` is a property of the **session**, not the renderer. It is chosen
from a fixed ladder (5s → 1800s) as the narrowest width that keeps the whole
series under 240 buckets, so a ten-minute session and a ten-hour one both
render legibly and two renderings of one session can never disagree about the
scale. Measured over 405 transcripts, the ladder has never bottomed out.

Buckets carry counts only. What the work *was* is deliberately absent —
segmentation and labelling are `phases` and `labels`, and neither belongs in
a bucket — and so is what *happened*: the turns and tool calls a bucket
counts are in the [events document](#the-events-document), keyed by time, so
a consumer that wants a bucket's contents, or buckets finer than the 240 this
series caps at, reads them there.

### `phases` — the session cut into stretches of work

Added in sprint 002, additively: nothing above moved.

```json
"phases": [
  { "started": "…", "ended": "…", "secs": 840, "span": 0,
    "kind": "implementing", "records": 96, "tool_calls": 61,
    "tool_failures": 2, "output_tokens": 41207,
    "mix": { "read": 12, "edit": 9, "run": 38, "org": 2,
             "ask": 0, "delegate": 0, "other": 0 },
    "opened_by": "fix the failing test" }
]
```

A phase is cut at **two** kinds of boundary, and both matter:

- **Every user turn**, because that is where the work was redirected.
- **Every idle break**, so a phase never spans a gap. `span` names the
  `activity.spans` entry it lies inside. Cutting only at user turns would let
  one phase quietly contain a three-day pause and report it as its own
  duration — the wall-clock lie one level up.

A phase runs until the next one **starts**, not until its own last record: the
seconds between an agent's last tool call and the user's next turn are real
work, and giving them to neither phase would make the durations fail to add up
with nothing on the page to show it. So **the phases of one span sum to exactly
that span's `secs`**, milliseconds and all.

Since 005 they also sum to `active_secs` exactly, because `active_secs` is now
read off the same spans. Before that it was `wall_secs - idle_secs` — two
truncations against the spans' one each — and the two disagreed by up to 198
seconds out of 12h39m on the corpus's 209-span session. The quantity being
counted did not change; where it is measured did.

`opened_by` is the preview of the user turn that opened the phase, and is
**absent** when the phase opens a resumed span instead: work picked up again
with nothing said. Absent is the honest reading; attributing it to the previous
prompt would be an invention.

#### `kind` and `mix`

`kind` is one of `exploring`, `implementing`, `running`, `filing`,
`delegating`, `discussing`, `mixed`. These name a **tool mix, not an intent** —
`implementing` means files were edited here, and `running` means mostly shell,
which under agent instructions that prefer shell editing may well be editing
kagviz cannot see. A descriptive label is a later, separate field written by a
model over these facts; it will never overwrite this one.

`mix` carries the counts `kind` was derived from, so the label can be checked
rather than believed. Tools are classified by a small table; MCP tools are
classified by their **operation**, matched exactly, so a file server (`read`,
`edit`) and a tracker (`list_work_items`, `create_work_item`) do not read as
the same activity. Anything unrecognised lands in `other`, which dilutes every
share equally rather than distorting one.

The thresholds live in `summary.rs` and are integer percentages, compared with
integer arithmetic — the same argument as `bucket_secs`, one step further: two
renderings of one session must not disagree about what a phase was, and a
float comparison is one platform difference away from doing so. The order the
rules are tested in is part of the rule; editing is deliberately cheap to earn,
because a change is almost always preceded by a lot of reading.

### `user_involvement` — the decision points

An ordered array, tagged by `kind`:

```json
{ "kind": "prompt",   "at": "…", "preview": "fix the failing test", "truncated": false, "attachments": 0 }
{ "kind": "question", "at": "…", "header": "Store", "question": "Which store?",
  "options": ["Postgres", "SQLite"], "chosen": "Postgres" }
```

`preview` is the first 80 characters of what the user actually typed,
whitespace collapsed — harness-injected context is excluded, which is the whole
point of the classifier. An empty `preview` with `attachments > 0` is the user
pasting an image or document and saying nothing.

A slash command reads as the line the user typed — `/start-sprint korg:1606
proceed with implementation` — rebuilt from the `<command-name>` and
`<command-args>` tags of the record the harness writes for it. That is exact,
where stripping boilerplate off the instruction document that follows would be
a guess at the same string.

`chosen` is absent when the transcript holds no answer. That is an interrupted
question, not a silent one, and it must not be rendered as a default choice.

## The events document

Added in sprint 009. **A third contract, beside the facts and `sessions.json`**,
and the detail tier under the facts: `kagviz show <id> --events` emits it,
`kagviz derive` writes it to `derived/events/<host>/<id>.json` beside the
facts, and `sessions.json` links it as `events`. It carries every assistant
turn and every tool call of a session, in time order, each stamped with the
phase that holds it — the things the facts' buckets and phases are counts
*of*. A click on a timeline segment reads this; nothing on the static report
does.

A separate document rather than a field of the facts, on purpose: a
twelve-hour session's facts are ~100 KB and its events run to megabytes, and
"forest, tree, leaf" wants the leaf fetched on demand. The same rules apply —
adding a field is not a breaking change, changing or removing one is, an
optional field is absent and never `null`, and nothing here is inferred.

**One pass, not two.** The events are built by the same accumulator that
produces the facts, so the two documents cannot disagree. The invariants a
consumer can lean on, and that the tests hold:

- `tool` events == the facts' `tool_calls` summed; `turn` events ==
  `assistant_turns`.
- `tool` events with `failed` == `tool_failures` summed **less `<unknown>`**.
  A failure whose call is not in the file has no call to hang on: the facts
  count it, the events cannot place it.
- `tool` events with `opaque` == `changes.opaque_edits`; `lines_added` and
  `lines_deleted` summed over the events are `changes.lines_added` and
  `lines_deleted`; the distinct `files` are `changes.files_touched`.
- For every phase `i`, the events with `phase: i` add up to that phase's
  `tool_calls` and `output_tokens` **exactly**. Measured over 413 transcripts:
  no phase of any session disagrees on either.
- **Failures do not add up per phase, and a consumer must not assert that they
  do.** The facts count a `tool_failures` on the record that carried the
  *result*; an event carries `failed` on the *call*, stamped with the call's
  own `at`. A call whose result came back after a phase boundary is therefore
  counted in one phase and drawn in the neighbouring one — in **either**
  direction, so a phase can place more failures than it counts. Summed across
  the phases the shortfall is exactly the `<unknown>` count, the same carve-out
  the session-level line has. Measured over 413 transcripts: 17 phases place
  more failures than their phase counts, and no session's total shortfall
  differs from `<unknown>`. The same reading applies to
  `activity…buckets[].tool_failures`, cut on the same timestamps.
- The same for each `spawns[k]` against `delegation.spawns[k]`.

This one was written the other way until sprint 012, when the app needed to
put both tiers on screen beside each other and the claim had to be true. It
was asserted in three places — this document, `tests/golden.rs` and the app's
`conformance.spec.ts` — and held in all three only because the fixture has no
straddling call. Corrected here and in both tests; **no value moved**, and no
document kagviz emits changed a byte. What changed is what the text promises.

```json
{ "session_id": "63a9b83b-…",
  "events": [
    { "kind": "turn", "at": "…", "phase": 3, "model": "claude-opus-5",
      "tokens": { "input": 1200, "output": 80, "thinking": 30,
                  "cache_read": 5000, "cache_write": 400 },
      "tools": 2 },
    { "kind": "tool", "at": "…", "phase": 3, "tool": "Edit", "class": "edit",
      "id": "toolu_…", "input_bytes": 412, "result_at": "…", "result_bytes": 66,
      "files": ["/home/ken/src/x/sync.sh"], "lines_added": 2, "lines_deleted": 1 },
    { "kind": "tool", "at": "…", "phase": 3, "tool": "Bash", "class": "run",
      "id": "toolu_…", "input_bytes": 90, "result_at": "…", "failed": true,
      "result_bytes": 4400, "opaque": true }
  ],
  "spawns": [ { "agent_id": "a3f518e6…", "events": [ "…" ] } ] }
```

| Field | Meaning |
|---|---|
| `events[]` | The session's own tier, in the order `activity` and `phases` were cut from: by time, ties in transcript order, then any event whose record carried no timestamp. A `turn` is followed directly by its `tool` events, in the order the message listed them. |
| `kind` | `turn` — an assistant message; `tool` — one call it made. Prompts and questions are not repeated here: `user_involvement` in the facts has them, with timestamps to merge on. |
| `at` | The record's timestamp. **Absent** when the record had none — and then so is `phase`. |
| `phase` | Index into the facts' `phases`. Absent on every event of a spawn — phases cut the *parent's* timeline — and on an untimestamped record. |
| turn: `model`, `tokens`, `tools` | The turn's model and usage (absent when the record carried none), and how many `tool` events follow it. |
| tool: `tool`, `class`, `id` | The tool's name; how the phase mix classified it — `read`, `edit`, `run`, `org`, `ask`, `delegate`, `other`, the same table `mix` uses; and the `tool_use` id, for joining back to the raw transcript. |
| tool: `input_bytes` | The call's input re-serialized compactly with sorted keys — a canonical size, not the on-disk one. |
| tool: `result_at`, `failed`, `result_bytes` | When the result was recorded; whether it came back `is_error` (present only when true); UTF-8 bytes of the result's text as the model was handed it. All three **absent** when no result arrived — an interrupted call, or one still running when the transcript ends. An offloaded result (`<persisted-output>`) counts its placeholder and preview, which is what the model saw; the harness's own `persistedOutputSize` is not carried yet. |
| tool: `files`, `lines_added`, `lines_deleted`, `opaque` | The call's file changes, under exactly the facts' two states. `files` are named when the result named them (absent when empty); the line counts are present when a diff was read and **absent** when not; `opaque` is present and true when this call is one of `changes.opaque_edits`. A shell call is opaque from the moment it is made — an interrupted one leaves no result and is still an edit kagviz cannot see. |
| `spawns[]` | One per `delegation.spawns[]`, same order, each with its `agent_id` and its own events. |

**What a consumer does with it.** Click a bucket: the events with `at` in
`[span.started + i·bucket_secs, +bucket_secs)`, plus the prompts and
questions from `user_involvement` in the same window. Click a phase: filter
on `phase`. Zoom past the strip's resolution: bucket the events yourself at
any width — which is why `MAX_BUCKETS` stayed at 240 when this document was
designed; the facts keep the resolution a static page needs and the app
derives its own. The facts' `records` per bucket is the one count the events
do not reproduce: it counts every timestamped record, `system` and snapshot
records included, and those carry nothing worth an event.

Not carried, deliberately: prompt and question text (the facts have them);
tool inputs and outputs themselves (the transcript has them, and this
document would be the transcript again); `system` records, hook summaries
and API errors (see `transcript-format.md`, "what else is recoverable" —
additive when wanted).
