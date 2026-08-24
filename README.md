# kagviz

Visualize how an agent session actually went.

A Claude Code session leaves a transcript on disk. kagviz reads it and reports
what happened: which tools ran and which failed, what changed on disk, where
the time went, and where the user was involved. The aim is **insight, not
debugging** — a picture of how the work got done, for the person who asked for
it.

The extraction is deterministic. Every number is a pure function of the
transcript bytes, so the same session yields the same report forever. Where a
model is used at all, it is only to *write the headline* over facts that were
already counted — never to produce the facts.

## Status

Early, but usable. The deterministic core reads transcripts and summarizes
them, `kagviz render` writes a self-contained HTML report, and the session is
cut into labelled phases. The optional model-written headline is next; see
`sprints/planning/roadmap.md`.

```console
$ kagviz sessions
SESSION                                PROJECT                 ACTIVE   TOOLS   FAIL
63a9b83b-9e7b-4f77-8ac2-df66ecd0407e   -home-ken-5090             38m      83      2

$ kagviz show 63a9b83b-9e7b-4f77-8ac2-df66ecd0407e
time      38m active / 2h30m wall (1h52m idle)
phases    44 (mostly running)
               5  running       12m
               3  mixed         8m
              26  discussing    5m
               4  filing        4m
files     3 touched, +57/-3  [28 opaque call(s) unaccounted]
              28  Bash  (28 unreadable)
               6  Edit  (3 file(s) +57/-3)
delegated 2 agent(s), 56 tool call(s), 35,663 out
            Explore       8 call(s)       2m    10,548 out  Summarize sprint deltas
            Explore      48 call(s)       5m    25,115 out  Map linking-layer code
combined  133 tool call(s), 4 failed, 406,894 out  (session + delegated)
```

`kagviz show <id> --json` emits the whole facts document. That JSON is the
contract: renderers consume it, and it is the seam a future interactive
front-end plugs into.

## The report

```console
$ kagviz render 63a9b83b-9e7b-4f77-8ac2-df66ecd0407e -o report.html
wrote report.html (52107 bytes)
```

One file, no external assets — no CDN, no web fonts, no scripts — so it opens
with no network, mails as an attachment, and still renders in five years. It
carries session identity, a headline row, the time strip with its phase bands,
where the time went by phase, the tool mix with failures, file changes, token
totals, and the moments the user was involved.

The renderer reads the **facts document**, never the transcript, which keeps
that seam honest and lets you render from a saved file or a pipe:

```console
$ kagviz show <id> --json > facts.json
$ kagviz render --from facts.json -o report.html
$ kagviz show <id> --json | kagviz render --from - > report.html
```

Because the facts are the only input, rendering is deterministic across
machines: the same facts document produces a byte-identical report on Linux and
Windows.

### The time strip

The strip is the answer to "where did the time go". Each column is a fixed
slice of time (the width is chosen per session and recorded in the facts, so it
cannot drift between renderings), bar height is record density, and a column
turns red where a tool call failed. Idle gaps are **collapsed to a hatched
break** with the duration on it, so a session resumed across a week reads as
several stretches of work rather than a week of whitespace.

Ticks above the columns mark where the user was involved. Green is a prompt;
amber is a question the agent stopped to ask — and the list below the strip
shows what was asked, what the options were, and which one was chosen.

### Phases

Above the columns runs a band of **phases**: the session cut into stretches of
work and each one named by its tool mix. The cut is at every user turn — that
is where the work was redirected — and at every idle break, so a phase can
never span a gap and report a three-day pause as its own duration.

The names are mechanical, and deliberately so. `implementing` means files were
edited here; `running` means mostly shell, which under agent instructions that
prefer shell editing may well *be* editing kagviz cannot see. They describe the
tools, not the intent. A descriptive label is a separate, later field written by
a model over these facts — it will never overwrite this one.

## Reading the output honestly

Two numbers deserve care, and both are there because leaving them out would
quietly lie:

- **active vs wall** — a resumed session can span days and hold an hour of
  work. Gaps of 2 minutes or more are counted as idle, not effort. Both are on
  the page: active is the headline, wall is its sub-label, because either one
  alone misleads.
- **phase labels name a tool mix**, never an intent. See above — this is the
  easiest thing here to over-read.
- **opaque calls** — an edit made through the shell leaves no recoverable diff,
  so the line deltas are a floor, not a total. The report says so on the page
  rather than showing a confident zero. `changes.by_tool` breaks it down per
  tool so the claim can be checked: which tools kagviz read exact numbers from,
  and which it could only count.
- **delegated work is a separate tier** — a subagent's tool calls and tokens
  are never folded into the parent's totals, because a session that spawned two
  agents really did make two `Agent` calls. Both tiers are shown with the sum
  spelled out. Active time is the exception and is *not* summed: a subagent runs
  while the session waits on it, so those seconds overlap rather than add.

If any transcript line fails to parse, the report opens with a banner saying
how many. A partial reading is never presented as a complete one.

`docs/transcript-format.md` documents the on-disk format, including the traps.
It is a field guide, not a spec: the format is undocumented and drifts between
CLI releases. `docs/facts-contract.md` documents the JSON the extractor emits
and the rules that govern changing it.

## Where it fits

- **harness-eval** scrapes session metrics ad hoc for its run logs; kagviz aims
  to be the thing that does that properly.
- **ai-findings** pairs field notes with infographics, and is the natural first
  consumer of a kagviz report.

## Development

Built on the [kprojects](https://github.com/kenhia/kprojects) minimal harness.

```sh
just          # list recipes
just check    # fmt + clippy + tests (the CI gate)
just fmt      # apply formatting
```

## License

MIT — see [LICENSE](LICENSE).
