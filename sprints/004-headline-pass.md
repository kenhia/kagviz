# Sprint 004 — the headline pass

korg:1586 · work item #1543 · branch `004-headline-pass`

## Goal

Let a model write *prose* over the facts — a session headline and a short
label per phase — without ever letting it write a *number*. This is the first
time a model is anywhere near a project whose stated rule is "facts are
computed, never inferred", so the deliverable is really the seam. The code is
small.

Three constraints, all load-bearing, all from #1543:

- **Strictly additive.** New fields only. Nothing existing moves, and no
  number is model-produced.
- **Cached.** Without a cache the report stops being reproducible, which
  forfeits the project's main property. The cache is not an optimization — it
  is the mechanism by which determinism survives contact with a
  nondeterministic component.
- **Opt-in.** A plain `kagviz render` stays a pure function of the transcript
  bytes.

## Decisions

### The labels are a sibling of the facts, not a field inside them

`Summary` grows one optional field, `labels`, and everything model-written
lives under it — including the per-phase labels, which are a **parallel array
keyed by phase index** rather than a `label` field on `Phase`.

That was the first real fork in the design, and it went the way it did on
purpose. Putting `label` on `Phase` would sit model-written text one line
below `tool_calls` and `mix`, in the same object, with nothing but the field
name to tell a consumer which is which. A parallel array makes the seam
physically visible in the JSON: everything under `labels` was written, and
everything outside it was counted. A consumer that ignores the whole `labels`
key gets exactly the document it got before this sprint, byte for byte.

### The cache key is the facts digest, and it decides everything

`facts_digest` is `sha256` over the serialized facts document with the
`labels` key removed. Every map in `Summary` is a `BTreeMap` and every list is
in a defined order, so that serialization is already stable across platforms —
sprint 001 has a test that says so, and this leans on it.

The property that buys: **facts identical → headline byte-stable forever;
facts changed → headline invalidated.** A cache hit needs no model at all, so
a rendered report re-renders identically with the GPU box switched off.

Two consequences worth stating rather than discovering:

- 002's and 003's additive changes to the facts would have invalidated every
  cached label. That is correct behaviour — a headline written over phases
  that did not yet include a delegation tier is a headline about a different
  session — but it means the cache is not a durable archive, and a facts
  change is a re-label.
- The **prompt is in the key too**, by content and not by version string. A
  changed prompt is a changed output; keying on a hand-maintained version
  number would make it possible to edit the prompt and silently keep stale
  labels. `prompt_version` is still carried in the output, as a human-readable
  label; the key does not trust it.

### Where the cache lives — a deviation from the proposal, stated

The proposal says "cached beside the transcript". Taken literally that means
writing into `~/.claude/projects/<slug>/`, which is the harness's own data
directory, and it does not survive the `render --from facts.json` path at all
— that path never knows a transcript.

So: `<transcript-root>/.kagviz/labels/<digest>.json`, overridable with
`--label-cache <dir>`. The transcript root is always known (`--root`, default
`~/.claude/projects`), so `--from` works the same way. It keeps the spirit of
"beside the transcript" — a corpus snapshot copied to another host takes its
labels with it, which an `~/.cache` location would not — without interleaving
kagviz's files with the harness's.

### Absent, not empty

No `--label` and no cache hit means the report has no headline block at all,
and the layout is designed around that being the *default* path rather than a
degraded one. Same rule as `opaque_edits`: an unknown is never rendered as a
zero, and here a headline nobody asked for is never rendered as an empty
string.

If `--label` is passed and the backend is unreachable, the report still
renders — without a headline — and stderr says why. Failing the whole render
because the GPU box is off would make the model a dependency of the
deterministic path, which is the exact inversion this sprint exists to
prevent.

### Marking model-written text on the page

A reader who cannot tell the headline from the counts has been handed the
confusion the project is built to avoid. So the written block is visually
distinct — its own accent rule, its own typeface treatment, no borrowing of
the `.stat` styling that numbers use — and it carries an explicit attribution
naming the model and saying, in words, that it was written rather than
measured. Phase labels get the same treatment in miniature: a marked chip
beside the mechanical `kind`, never replacing it.

The footer's old claim ("nothing here is inferred") becomes a lie the moment a
headline is on the page, so it changes to say which parts are which.

### Backend

`Labeler` is a two-method trait; `Kvllm` is the one implementation, talking to
the OpenAI-compatible `/v1` kvllm serves on kai (`--label-url`, default
`http://localhost:8000/v1`, env `KVLLM_BASE_URL`). `model=auto` asks
`/v1/models` what is loaded, matching kvllm-client's convention — kvllm serves
exactly one model, so consumers follow when the served model changes.
`temperature=0`, fixed seed. That is not enough for byte-stability on its own,
which is the whole reason the cache exists rather than a nice-to-have.

`ureq` rather than `reqwest`: kagviz is a synchronous CLI and does not need an
async runtime to make one request per render.

## What the corpus changed about the design

Two of the decisions above are corpus findings, not desk work. Both were
invisible in the unit tests and obvious on the first real sweep.

### Positional labels do not survive a real session

The first cut asked for `"phases": ["label", "label", …]` and mapped by
position. On the corpus the median session has a handful of phases — and the
worst has **392**. Drop the eighth label of 392 and every phase after it wears
someone else's sentence, with nothing on the page to show it. That is the worst
possible failure here: not a missing label, but confident prose attached to the
wrong stretch of work.

So the protocol is numbered: `{"phase": 3, "label": "…"}`, and a number naming
no phase is discarded rather than guessed at. A bare array is still accepted,
but **only** when its length exactly equals the phase count — at that length
position is unambiguous, and at any other length the labels are dropped rather
than mapped on a guess.

That change is also what made the next one safe.

### A 392-phase session is a 44 KB prompt

Measured across 405 briefs before the fix: median 1,220 characters, **maximum
44,527**, asking for 392 labels. Past a local model's context, and well past
where the labels stay aligned — and the report only ever lists the 15 longest
phases anyway, so most of that prose would have been paid for and never read.

The brief now carries at most 24 phases, chosen **by duration** rather than by
position, and *says* that it truncated so the model does not write a headline
over a third of a session as if it were the whole one. The numbered protocol is
what makes a non-contiguous list safe to send. After: median 1,220, maximum
10,823, at most 24 phases.

### And one that only a real session could show

A user's answer to an `AskUserQuestion` can run to paragraphs — one in the
corpus does. Passed through raw it stopped being one line of a list and became
loose multi-line text sitting in the prompt: noise, and an injection surface.
Quoted spans are now whitespace-collapsed and bounded.

## Shipped

- **`labels`, the one model-written field.** Additive, absent without
  `--label`, and physically separate from the counts: phase labels are a
  parallel array keyed by phase index, so a consumer that ignores the key gets
  the pre-004 document exactly.
- **`--label`, `--relabel`, `--label-url`, `--label-model`, `--label-cache`**
  on both `show` and `render`. `--label-model auto` asks `/v1/models` what is
  served, following kvllm-client's convention.
- **The no-quantities brief.** The mechanism, not a policy: the model is handed
  ranked tool *names*, ordinal phase sizes and the user's own words, and no
  measurement at all. A test asserts it on a fixture; the corpus sweep asserts
  it on 405 real sessions.
- **The cache**, keyed on `sha256(facts)` plus the prompt's own bytes, in
  `<root>/.kagviz/labels/`. The model id is deliberately *not* in the key — see
  the decision above.
- **The marked report.** Written text gets its own accent, its own serif face,
  a `written` chip on every occurrence, and an attribution naming the model.
  The phase list shows the written label *beside* the mechanical `kind`, never
  instead of it. The footer stopped claiming nothing on the page is inferred,
  because with a headline on it that was false.
- **`prompts/headline.v1.md`**, versioned in the repo and hashed into the cache
  key, so an edited prompt cannot silently keep stale labels.
- Docs: `docs/facts-contract.md` gained the `labels` section; README gained
  "The headline (opt-in)".

### Verified

- `just check` green: 71 tests, clippy clean over `--all-targets`.
- **Strictly additive, measured**: all **405** corpus transcripts across kai,
  kubs0 and cleo produce facts **byte-identical** to the pinned sprint-003
  baseline at `3823617`, and not one plain `show --json` emits a `labels` key.
- **No computed quantity reaches the model**, over all 405 briefs. Every digit
  that does is inside the user's own quoted words or part of a recorded name
  (`mcp__kaed-kubs0__read`, `deploy-kubs0`, `git branch: 003-curator`).
- **Byte-stable**: three renders of one labelled session — fresh, cached, and
  cached with the backend unreachable — are byte-identical.
- **Absent, not broken**: `--label` with the backend down renders a report
  byte-identical to the unlabelled one and warns on stderr.
- `render --from facts.json` renders labels that ride the document, with no
  model call, identically to the direct render.

### Not verified against a real model

kvllm could not be started: **kai's GPU is out for RMA.** (`nvidia-smi` fails and
`/proc/driver/nvidia` is absent; DKMS still has `595.58.03` built for the running
kernel, which is what made this look at first like a post-upgrade driver reload.
It is not — the card is physically gone, and no amount of modprobe fixes that.)
Everything
above was exercised against a stub speaking the same OpenAI-compatible `/v1`,
including model discovery, the request shape, fenced replies and the audit of
every brief. What remains untested is **prose quality** from a real 7B — whether
the headline is worth reading, and whether the model honours the numbered-label
protocol without drifting. That is a live-fire check, not a correctness one.

## Follow-ups

- **Run it against real kvllm** once kai has a GPU again, and judge the prose.
  Expect prompt iteration; the cache key hashes the prompt, so editing it
  invalidates cleanly.
- **Skill-invocation previews swamp `opened_by`.** On real sessions a large
  share of phase openers read `"Base directory for this skill: /home/ken/.claude/skills/start-sprint # Start Spr"` — 80 characters of harness boilerplate instead
  of what the user asked for. That is a *facts* defect, not a labelling one: it
  degrades the Phases panel today and it starves the headline pass of its best
  evidence. Worth its own work item against `PREVIEW_CHARS` / the injected-text
  classifier.
- **`git branch: HEAD`** appears in the facts for detached-head sessions and is
  passed to the model as if it were a branch name. Harmless, but it is a string
  that means "unknown" being reported as a value.
