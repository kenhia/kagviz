# The facts document

`kagviz show <id> --json` emits one JSON object: everything kagviz was able to
count about a session. It is the **only** input the renderer takes, and it is
the seam a future interactive front-end plugs into.

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
| `models` | model id → assistant turns on that model. |

Time — all three are reported because any one alone misleads:

| Field | Meaning |
|---|---|
| `started`, `ended` | First and last timestamped record, RFC 3339 UTC. |
| `wall_secs` | `ended - started`. |
| `idle_secs` | Sum of inter-record gaps of `IDLE_GAP_SECS` (120s) or more. |
| `active_secs` | `wall_secs - idle_secs`. |

Volume and outcome:

| Field | Meaning |
|---|---|
| `records` | Transcript lines parsed. |
| `skipped_lines` | Lines that did **not** parse. Non-zero means every number here is partial, and the report says so on the page. |
| `assistant_turns`, `user_prompts` | `user_prompts` counts real user turns only — see the `promptId` trap in `transcript-format.md`. |
| `tool_calls`, `tool_failures` | tool name → count. Failures are joined to their call by `tool_use_id`; a failure whose call is not in the file is blamed on `<unknown>` rather than dropped. |
| `tokens` | input / output / thinking / cache read / cache write. |
| `changes` | `files_touched`, `lines_added`, `lines_deleted` recovered from a diff, plus `opaque_edits` and a per-tool `by_tool` breakdown. |
| `skills`, `subagents`, `subagent_transcripts` | What the session delegated to. The delegated *work* is in `delegation`. |
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

- Every `Bash` / `PowerShell` call, counted from the call rather than from a
  result — an interrupted shell call leaves no result and is still an edit
  kagviz cannot see.
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

**`active_secs` does not combine.** A subagent runs while the session waits on
it, so those seconds overlap rather than add. There is deliberately no
`combined_active_secs`; tokens add across concurrent agents, seconds do not.

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
scale. Measured over 305 transcripts, the ladder has never bottomed out.

Buckets carry counts only. What the work *was* is deliberately absent:
segmentation and labelling are later work, and neither belongs in a bucket.

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

They do *not* sum to `active_secs`. That difference is older than this field:
`active_secs` is `wall_secs - idle_secs`, two truncations, while the spans
truncate once each — on the corpus's 209-span session the two disagree by 198
seconds out of 12h39m. Phases inherit it; they did not introduce it.

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

`chosen` is absent when the transcript holds no answer. That is an interrupted
question, not a silent one, and it must not be rendered as a default choice.
