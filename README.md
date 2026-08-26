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
them, `kagviz render` writes a self-contained HTML report, the session is cut
into labelled phases, and an opt-in `--label` pass lets a model write the
headline over facts already counted. On the homelab, a nightly collector
mirrors every host's transcripts before the CLI prunes them and derives a
cross-host session index from the mirror, and a static app over that index —
sortable, filterable, a page per session — is served from the same tree. See
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
contract: renderers consume it, and it is the seam the front-end plugs into —
typed in `web/src/lib/contract/` and held to the goldens by a conformance test
that runs inside `just check`, so a facts change that breaks the app fails the
Rust build the day it lands. `kagviz show <id> --events` emits the detail tier under
it — every turn and tool call, joined to its phase, with sizes, outcomes and
the files each call changed — as a separate document, so the facts stay light
and a click on the timeline has something to read.

## The report

```console
$ kagviz render 63a9b83b-9e7b-4f77-8ac2-df66ecd0407e -o report.html
wrote report.html (52107 bytes)
```

One file, no external assets — no CDN, no web fonts, no scripts — so it opens
with no network, mails as an attachment, and still renders in five years. It
carries session identity, a headline row, the time strip with its phase bands
(and, on a dense session, a zoom-in checkbox — CSS only, still no script),
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

A band names itself in the band where there is room for the word, and on a
session with hundreds of stretches there is not. Below that width the band
keeps its colour and its tooltip rather than showing a clipped fragment; the
phase list beneath the strip is the key.

Each phase is opened by the user turn that started it, and a slash command
reads as the line that was typed — `/start-sprint korg:1606 proceed with
implementation`, not the instruction document the harness hands the agent when
a skill runs. Telling those two apart is `docs/transcript-format.md` trap 5,
and getting it backwards is worth a look at that page before touching the
classifier.

## The headline (opt-in)

Everything above is counted. `--label` adds the one thing that is not: a model
writes a sentence over the session and a short label per phase.

```console
$ kagviz render <id> --label -o report.html
labelled by qwen2.5-7b-instruct (headline.v1)
```

Off by default, and that is the contract — a plain `render` stays a pure
function of the transcript bytes, and a facts document nobody labelled has no
`labels` key at all.

Three things make it safe to have at all:

- **It is never shown a number.** The digest handed to the model carries no
  counts, no durations, no token totals — ranked tool *names*, ordinal phase
  sizes, and the user's own words. A model that never sees a measurement cannot
  write one that contradicts the panel below it.
- **It is cached on the facts.** Same facts, same sentence, forever; a cache hit
  never contacts the model, so a labelled report re-renders with the model host
  switched off. Change the facts and the labels are re-written rather than
  reused, because they described a different session.
- **It is marked on the page.** The headline gets its own accent, its own face,
  and an attribution naming the model; phase labels get a marked chip beside the
  mechanical `kind`, never instead of it. The footer says which text is which.

If the backend is unreachable the report renders **without** a headline and
stderr says why — an absent headline, not a failed render, because a model must
never become a dependency of the deterministic page.

```
--label-url    OpenAI-compatible base URL (default: $KVLLM_BASE_URL, else localhost:8000/v1)
--label-model  model id; `auto` (default) asks the backend what it serves
--label-cache  where cached labels live (default: <root>/.kagviz/labels)
--relabel      ignore the cache and ask again
```

The prompt is versioned in the repo under `prompts/` — a changed prompt is a
changed output, so it lives in git, and its bytes are part of the cache key.

## Reading the output honestly

Two numbers deserve care, and both are there because leaving them out would
quietly lie:

- **active vs wall** — a resumed session can span days and hold an hour of
  work. Gaps of 2 minutes or more are counted as idle, not effort. Both are on
  the page: active is the headline, wall is its sub-label, because either one
  alone misleads.
- **phase labels name a tool mix**, never an intent. See above — this is the
  easiest thing here to over-read. A `--label` run adds a *written* label
  beside the mechanical one; the marked chip is the difference, and it is
  marked on every occurrence for exactly that reason.
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

## Collection

The CLI prunes `~/.claude/projects` after ~30 days, so on its own a session
report is a thing you can make for a month. `collect/` keeps history:

```console
$ just collect            # sync kai, kubs0, cleo into /ai-data/kagviz-data/live, then derive
kai    ok              12 file(s)    3s
kubs0  ok               0 file(s)    2s
cleo   unreachable      0 file(s)    0s  did not answer ssh
kai        199 session(s)      2 derived    197 unchanged
kubs0       93 session(s)      0 derived     93 unchanged
cleo       113 session(s)      0 derived    113 unchanged
index      405 session(s) → /ai-data/kagviz-data/live/derived/index.html
```

An **accumulating mirror** per host (never pruned, never written by kagviz),
and under `derived/` the facts, a report, and a cross-host `sessions.json` +
`index.html` regenerated for whatever changed — by content hash, and in full
whenever the kagviz that derived it changes. A host that is asleep is
recorded as *unreachable* on the page rather than read as "nothing new". A
systemd user timer on kai runs it at 04:00 Pacific, and copyparty serves
`derived/` on the tailnet — the page is
`https://kai.encke-wahoo.ts.net:8027/kagviz/index.html` (the bare `/kagviz/`
is copyparty's directory listing).

`kagviz derive` and `kagviz index` are the two subcommands behind it;
`docs/collection.md` has the layout, the mechanism per host, and the
operating notes. `sessions.json` is a contract like the facts —
`docs/facts-contract.md` documents it.

## The app

`web/` is a static single-page app over the same three documents: the index,
the facts, and (in part 2) the events. No backend — it is HTML, CSS and JS
copied next to the data on copyparty, at
`/kagviz/app/index.html`. It carries the report's panels plus what a static
page cannot: sorting, filtering, and a page per session reachable by URL.

```sh
just web-check     # lint, svelte-check, build, vitest — what CI runs
just web-dev       # the dev server
just web-deploy    # build and install at derived/app/ on the served tree
```

The static report is unchanged and stays. `web/README.md` has the decisions —
why the router is hash-based, why the bundle lives under `derived/`, and the
two traps that only showed up on deploy.

## Where it fits

- **harness-eval** scrapes session metrics ad hoc for its run logs; kagviz aims
  to be the thing that does that properly.
- **ai-findings** pairs field notes with infographics, and is the natural first
  consumer of a kagviz report.

## Development

Built on the [kprojects](https://github.com/kenhia/kprojects) minimal harness.

```sh
just              # list recipes
just check        # the CI gate: Rust (fmt + clippy + tests) and the app
just rust-check   # the Rust half alone
just web-check    # the app's half alone
just fmt          # apply formatting, both halves
```

A gate that skips the app is a gate that lies, so `just check` runs both. The
app's half needs Node; it installs from `web/package-lock.json` on first run.

## License

MIT — see [LICENSE](LICENSE).
