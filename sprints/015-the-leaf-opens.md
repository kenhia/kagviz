# 015 — the leaf opens: read what the call actually said

korg:1659 · #1656, #1657, #1658

Sprint 012 built the leaf: click a timeline segment and the panel gives you
the turns and tool calls behind it, each row carrying time, tool, class,
duration, bytes in and out, `failed`, `opaque`. What it cannot tell you is
what the command *was*. That is not an oversight — the events document says
outright that it carries no payload text, because "this document would be the
transcript again".

This sprint adds the fourth document, `calls/<host>/<id>.json`, and opens the
row into it.

## The premise the proposal was raised with, and what measuring did to it

The feature was expected to need a service on the host, "as we will be dealing
with bigger chunks of data". Measured over the live mirror before proposing,
that is wrong, and the reason is the interesting part: **the harness already
bounds the payloads.** A result too large for the context is offloaded to
`tool-results/<id>.txt` and the transcript keeps a `<persisted-output>`
placeholder, so the inline text is pre-capped.

    90,182 payloads across 413 sessions
    inputs 38 MB + results 66 MB = 103 MB   (17.9% of 576 MB of transcript)
    individual payload  median 308 B  p99 11.8 KB  max 85 KB
    payloads over 100 KB                    0

Per session as one static file: median 190 KB, max 4.0 MB — against an events
document the app already fetches at median 42 KB, max 1.7 MB. Same class of
artefact, not a new one. So: no engine, and `derived/` keeps the property that
makes the whole thing cheap — disposable, regenerable in seconds from the
mirrors, and one binary a timer runs.

## The gate was disclosure, and it was decided at the start of the sprint

Everything kagviz serves today is *derived*: counts, durations,
classifications, 80-character prompt previews. The raw mirrors are not
reachable at all. Call text would be the first raw session content on a served
surface — and the corpus README is explicit about what that content is: "file
contents, command output, pasted material, and potentially credentials. They
live here and stay here."

Scanned all 90,182 payloads: ~51 plausibly-live credentials, plus 172
placeholder DSNs, touching **59 of 413 sessions**.

### Two knobs were being conflated, including in the question first put to Ken

1. **Does the `calls/` document exist at all?** This is what "off by default"
   means, and it is *not* about removing sensitive bits. `derive` writes
   `calls/` only when asked; nothing is cleaned or filtered, the file is
   either created or it is not. The reason is blast radius: `collect/demo.sh`
   invokes a bare `kagviz derive --live "$TREE"`, so a default-on `calls/`
   would put raw call text in **every** demo without anyone having decided to.
   Off-by-default makes the flag *be* the decision.
2. **Is anything removed from the text once it exists?** Redaction — a
   separate question, and the one #1657 spends its length warning about.

### What was decided (Ken, at the start of this sprint)

- `kagviz derive --calls` writes the document. The nightly timer runs plain
  `derive`, so the tailnet-served tree is unchanged unless someone asks.
- `just demo --calls` passes the flag through. **This corrected a restriction
  that was the agent's, not the sprint's**: the option first put to Ken said
  "tailnet gets calls, the demo tree never does", and Ken's objection was the
  right one — he wants full fidelity in a demo, and there is no reason he
  cannot have it. Default off in *both* trees, available on demand in *both*.
  The demo is the higher-risk surface, so default-off matters more there — but
  it is one flag away, and the presenter is already hand-picking the glob.
- **No redactor.** Rejected for the reason #1657 states: a scanner that
  catches 51 shapes and misses the 52nd manufactures false confidence, the
  same unknown-rendered-as-a-zero this project refuses everywhere else.
  Recorded as a decision, not skipped silently.
- **Local-only rejected** — it would gut #1658, whose whole point is the app.

### The addition neither option offered: a floor-reporter, not a redactor

`just demo --build-only` is already the pre-check and already prints what is
in the tree. It now also prints how many payloads in the **selected** corpus
match known credential shapes — stated explicitly as a **floor**, never as a
clearance, so `0` reads as "this scan found none of the shapes it knows about"
and never as "clean".

That is `opaque_edits` discipline applied to the pre-check. It is the honest
half of scanning: it *informs* the human check sprint 014 established rather
than pretending to replace it, which is exactly the trap that made a redactor
wrong. The distinction is the whole point — **a redactor's clean pass is a
claim about the text; a floor-reporter's zero is a claim about the scanner.**

## What shipped

**#1656 — the calls document.** `derived/calls/<host>/<id>.json`, joined to the
events by `tool_use_id`. Built by the same `Counter` that produces the facts
and the events, off the same block in the same loop iteration — which is what
makes `input`/`result` agree with `input_bytes`/`result_bytes` by construction
rather than by assertion. Flat across tiers: the session's calls and every
spawn's in one list, because the id is unique per session and a consumer
expanding a delegated agent's row should join exactly as it does the parent's.

Three fields exist only because a zero would otherwise stand in for an
unknown, and the corpus says none is theoretical:

| | over 405 sessions / 45,394 calls |
|---|---|
| `result` **absent** vs `""` — interrupted vs a real empty result | **58** interrupted |
| `result_blocks` — types that carried no text | **4,672** (`tool_reference` 4,424, `image` 248) |
| `persisted` + `persisted_bytes` — the preview is not the output | **19**, all with a size |

`persisted_bytes` also closes something the events contract had flagged as
"not carried yet": the harness's `persistedOutputSize`. One real case is a
2,224-byte preview standing in for 227,547 bytes of PowerShell output. Read
from `toolUseResult.persistedOutputPath`, never by matching the shape of the
text — the harness says what it wrote, and that beats a regex over it.

**#1657 — the decision**, recorded above and on korg:1657.

**#1658 — the leaf opens.** `Segment.svelte` grows a second expand on a tool
row. Fetched **lazily**, and only on a reader's first click: nothing touches
the calls document on page load, so a reader who never opens a call pays
nothing for a document that is ~4.5× the events at the median.

The first open costs two fetches, and the second one is the interesting one.
`sessions.json` is read **not to find the path** — that is
`calls/<host>/<id>.json` and always was — but to find out whether there is
anything at it. Reading a 404 as "this tree carries no call text" would
conflate the deliberate default with a derive that half-finished. So the
panel can say *"this tree carries no call text — `kagviz derive --calls`
writes it"* as a fact rather than a guess.

The presentation rules live in `web/src/lib/calltext.ts`, a pure module with
its own tests, not in the markup. That is deliberate: the rules are the ones
the contract cares about, and **a rule that lives in markup is a rule nothing
tests.**

## What the pre-check now shows, and why it is the whole argument

One real project, 22 sessions, the same corpus derived both ways:

```
             default          --calls
scanned      70 files         92 files
matched      0 of 5 shapes    81:  private-key 5  sk-ant 15  KEY=value 58  dsn-password 3
```

That is off-by-default justified in one screen, in the place the presenter is
standing. Each of the five patterns was verified to fire against a planted
sample and to return to zero when it was removed — an untested scanner's zero
is worth nothing.

## The defect this sprint found in itself

The conformance test passed. Then the app's own decoders were run over a real
derived tree, and the byte invariant failed: `result.length` 2,943 against
`result_bytes` 2,959.

**`String.length` counts UTF-16 code units; every size kagviz emits is UTF-8
bytes**, because Rust's `str::len()` is. They agree on ASCII and part company
everywhere else — measured after the fact, **6,093 of 11,819 corpus tool
results disagree**. The documented invariant was wrong for over half of real
payloads, and the app would have printed a byte figure beside the events'
that quietly differed.

It passed because `tests/fixtures/` did not contain **one non-ASCII byte**, so
`.length` was correct there and the test could not have failed. The fixture
now carries an em-dash and a `µ` in a tool result and an input, the goldens
moved by exactly those two payloads' sizes, and the conformance suite asserts
the fixture *still* has a payload where the two measures disagree — so the
test cannot silently become unfalsifiable again.

The lesson generalises past this bug, and it is the second time the project
has paid for it: sprint 012's phase-failure claim held in three places "only
because the fixture has no straddling call". **A fixture that cannot express
the failure makes the test that checks for it meaningless.** Worth asking of
any invariant this project adds: what would the fixture have to contain for
this test to be able to fail?

A related collision was caught on the way: the expanded input's "show all"
button would have printed the *formatted* JSON's size directly beneath the
row's `input_bytes`, which the contract defines as the **canonical compact**
serialization. Two different numbers for one input, side by side, is the
disagreement this panel renders as a warning everywhere else — so that button
carries no figure at all. The result's does, because there the text on screen
and the text measured are the same bytes.

## Verification

- `just check` green: 115 unit + 12 golden + 107 web tests.
- **Corpus sweep, 405 pinned transcripts**: facts and events **byte-identical**
  to the 013 baseline `b0edcaa` on every one — 0 files differ on either tier,
  across all three hosts. The change is additive, measured rather than argued.
- The calls document over the same 405: **45,394 calls, zero invariant
  violations** (`.scratch/015/callcheck.py`). Median 204 KB a session, p90
  594 KB, max 4.5 MB, 114 MB total — against the 190 KB / 103 MB the proposal
  projected from raw payload bytes before any of this existed.
- The app's real decoders and view logic over a real derived tree: 10
  sessions, **1,385 tool rows opened, 0 unjoined**, and the byte count the
  panel prints equal to `result_bytes` on every one.
- `--calls` → `--drop-calls` → `--calls` round-tripped on a real tree, with
  `sessions.json` linking and un-linking to match.
