# The facts document

`kagviz show <id> --json` emits one JSON object: everything kagviz was able to
count about a session. It is the **only** input the renderer takes, and it is
the seam a future interactive front-end plugs into.

Treat it as a contract:

- **Adding a field is not a breaking change.** Consumers must ignore fields
  they do not know.
- **Changing or removing a field is.** So is changing what an existing field
  counts.
- **Every value is computed from the transcript bytes.** Nothing in here was
  inferred, estimated, or written by a model. A model may one day add a
  *headline* over these facts; it will be a new field, and it will never
  replace one.
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
| `changes` | `files_touched`, `lines_added`, `lines_deleted` from `structuredPatch`, plus `opaque_edits`. |
| `skills`, `subagents`, `subagent_transcripts` | Delegation. Subagent transcripts are counted but **not** folded in yet. |

### `changes.opaque_edits`

Shell calls that could have changed files and left no recoverable diff. These
are **not** added to the line deltas — they are the count of edits kagviz
cannot see. `lines_added` is a floor, not a total, and under agent instructions
that prefer shell editing the shortfall is systematic rather than occasional.

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
