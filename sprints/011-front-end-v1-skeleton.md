# Sprint 011 — front-end v1, part 1: the skeleton, the contracts in TypeScript, the session browser

korg:1641 · covers #1636, #1637, #1638 · first half of #1619

## Goal

An app over the facts. Not the interesting half — no pan, no zoom, no click
into a segment; those are part 2 (#1591, #1639) and carry a different risk.
This half is plumbing: a stack, a static host that serves files and not SPA
fallbacks, somewhere for the bundle to live, three contracts typed without
drifting from the Rust, and web gates inside `just check` so CI keeps telling
the truth. The bet was that landing it first means part 2 starts from a
deployed app with typed data and a conformance test already in the gate — and
that the browser is useful on its own the day it ships.

It is. Sorting and filtering 407 sessions across three hosts is a thing the
static `index.html` could never do.

## What shipped

**`web/`** — SvelteKit + Svelte 5 + TypeScript, `adapter-static`, hash router.
Two pages: the session browser at `#/` and a session page at
`#/s/<host>/<id>`. Deployed to `derived/app/` and served at
<https://kai.encke-wahoo.ts.net:8027/kagviz/app/index.html>. 212 KB built.

**The three contracts in TypeScript** (`web/src/lib/contract/`), decoded rather
than cast, with a conformance test over the repo's goldens that runs inside
`just check` and CI.

**`just check` is now `rust-check` + `web-check`.** A gate that skips the app
is a gate that lies.

The static report is unchanged and stays.

## Decisions

### Hash routing, and the two things it buys

copyparty serves files. A GET for `/kagviz/app/s/kai/<id>` is a 404, not
`index.html`, so a path-routed SPA needs a fallback the server will not give
it. `router.type: 'hash'` puts the route in the fragment, which the server
never sees, and one `index.html` is the whole app.

The second thing it buys was not in the proposal and turned out to matter more:
the **document** URL never changes as you navigate. So `fetch('../sessions.json')`
resolves the same way on the browser page and on a session page — the app finds
its data by being *next to* it, with no base path configured anywhere. A
path-routed SPA at `/kagviz/app/s/kai/<id>` would have to know how deep it was.

### `derived/app/`, on the data's own origin

No CORS and no k-homelab manifest change: copyparty already serves `derived/`
at `/kagviz/`. `just web-deploy` builds and stages the bundle in, renaming into
place the way `derive` writes every file, so a half-copied bundle is never
served. `derive` and `index` never write there.

`app/` is the one thing under `derived/` a *run* does not rebuild — it comes
from the build, not from `derive`. Still regenerable from the same checkout,
which is what that rule is for. `docs/collection.md` says so where the rule is
stated.

The static `index.html` links the app **only when `app/index.html` is actually
there** (`derive::APP_ENTRY`, with a test). A link to a 404 leaves the reader
unable to tell "not deployed" from "broken" — the same distinction the sync
line one paragraph above it exists to keep visible.

### Node in CI

`actions/setup-node@v7`, pinned by sprint 010's method rather than from memory:
`runs.using` read from `action.yml` at each ref through the API (v4 `node20`;
v5, v6, v7 all `node24`), then the later majors' release notes read against how
this workflow actually uses it — v6 limits automatic caching to npm (we use
npm), v7 is ESM plus new cache outputs. Neither touches us, so the latest
compliant major wins.

## Two traps, both found by deploying

Neither showed up in the local smoke test against `python3 -m http.server`.
Both are properties of *this* host, and both are now written down where the
next person will hit them.

### 1. The hash-routing shell ships with absolute asset URLs

SvelteKit's `paths.relative` rewrites asset URLs on a page it **prerenders**,
because it knows how deep that page sits. The hash-routing shell is generated
as a *fallback* — a page with no path of its own — so kit deliberately leaves
its `<link>` and `import()` URLs absolute (`/_app/…`) and makes only the
runtime base relative (`kit/src/runtime/server/page/render.js`, the
`state.prerendering?.fallback` branch). Mounted at `/kagviz/app/` those resolve
against the server root and the app never boots.

Hash routing is exactly the condition that makes the fix safe: the shell is
always `index.html` at the deployment root and its URL never changes, so
`./_app/…` is correct wherever the directory is copied. `web/scripts/relativize.js`
does the rewrite as `postbuild` and **throws** if it finds nothing to rewrite
or anything left absolute — a kit release that changes this output fails the
build rather than shipping a page that silently cannot start.

The bundle is therefore mount-independent: it works at `/kagviz/app/`, under
`npm run preview`, and from a plain `file://` open, with nothing baked in.

### 2. `resolve()` from `$app/paths` is unusable here

It returns `base + '#' + path`, and `base` is the runtime-computed *directory*
— so every row's href came out `/kagviz/app#/s/kai/<id>`. That is a link to the
directory, not to the shell inside it, and copyparty serves a directory as a
file listing (the same trap `docs/collection.md` already recorded for
`/kagviz/`). Clicking a session left the app for a file listing.

Hrefs are bare `#/…` fragments now, resolved against `document.baseURI`.
`browse.spec.ts` pins it: the href must start with `#` and must not start with
`/`. The one eslint-disable in the tree is on that line, with the reason.

## The conformance test, and the defect it found

`web/src/lib/contract/conformance.spec.ts` reads the bytes the Rust binary
actually emits — `tests/golden/fixture-0001.{facts,events,sessions}.json` —
puts them through the decoders, and asserts what `docs/facts-contract.md`
states.

`fixture-0001.sessions.json` is new: rather than hand-build an index row in the
test, the derive golden test now writes one, normalising only the `kagviz`
version stamp (it moves every commit) and asserting the placeholder still
matches what derive emits, so it cannot rot into a lie.

**On its first run it failed, and the document was wrong.** The events section
claimed:

> For every phase `i`, the events with `phase: i` add up to that phase's
> `tool_calls`, `tool_failures` and `output_tokens`.

with no `<unknown>` carve-out — three lines below the session-level bullet that
has one. The fixture has a counterexample: phase 4 counts one `tool_failures`
the events cannot place, because that failure's call is not in the file. Both
sides are behaving correctly — a phase must not report an unknown as a zero,
and the events have no call to hang it on — and the *contract text* was the
thing that was wrong. `tests/golden.rs` had only ever checked `tool_calls` per
phase, so nothing caught it.

Fixed in the document, and now asserted by **both** consumers, with the
shortfall required to equal the `<unknown>` count exactly so it cannot be
satisfied by some other bug.

That is the case for the test existing. It ran once and repaid itself.

### What else it holds

Absent-vs-null is split the way the contract actually states it: the
**decoders** fold `null` into `undefined`, because the contract tells a
*consumer* to read the two alike; the **test** asserts the golden bytes contain
no `null` at all, which is the producer's half. Unknown fields are ignored
(adding one is not a breaking change); an unknown `kind` throws, because a
kagviz newer than the app is better as a loud failure than as a guess.

The derived helpers make the report's own two choices and are checked against
`fixture-0001.show.txt`'s numbers rather than numbers restated in a test: the
failure rate is 1/16 = 6.25%, not 2/16, and the combined line is 19. There is a
test asserting **no** `combined*Active*` helper exists, so the day someone adds
one they read why seconds do not add.

## Three defects found by looking

All three were invisible to the tests and obvious on screen. Worth naming
because the ratio is the lesson.

1. **The strip rendered only idle** on the corpus's 209-span session. Breaks
   are a fixed width and spans have no width of their own, so 208 breaks
   claimed more than the whole strip and every span was squeezed to zero: the
   panel whose job is collapsing idle was showing nothing but idle.
   `render.rs` already solved this in sprint 008 with three densities and a
   comment saying why; the app now uses the same thresholds and there is a test
   on the 209 case. 53% work / 47% break marks, same as the report.
2. **"41 failed."** A tool with 4 calls and 1 failure rendered them adjacent —
   the space inside the `<em>` was collapsed away by the formatter. That is a
   wrong number on the page, not a cosmetic slip. Margin, not whitespace, in
   both places it occurs.
3. **Column widths.** The project `<select>`'s longest option pushed the whole
   page wider than a phone; the What column, unbounded and then over-corrected
   to a few glyphs by `overflow-wrap: anywhere`, pushed the `report` link off a
   1400px screen. The caps and floors sit on blocks *inside* the cells —
   `max-width` on a `td` is advisory under `table-layout: auto` — and the
   redundant " UTC" moved from 407 rows to the column header.

## What is deliberately not here

- **Pan, zoom, click.** Sprint 012. The strip is drawn once at the session's
  own `bucket_secs`; finer buckets come from the events document, which is why
  `MAX_BUCKETS` stayed at 240 when that document was designed. The events
  document is typed and held to its invariants here so part 2 starts from a
  proven shape.
- **A responsive card layout for phones.** The browser's table scrolls inside
  its own container on a narrow screen; the page itself never does. Half-width
  — how Ken works — reads well, and that was the bar. A phone-shaped list is a
  follow-up, not a blocker.
- **Replacing the static report.** It stays. The session page links it.

## Deployed

`just web-deploy` on kai, 2026-08-26, into
`/ai-data/kagviz-data/live/derived/app/` (212 KB), then `kagviz index` to
regenerate the browse page so it carries the link.

Verified live over copyparty at
<https://kai.encke-wahoo.ts.net:8027/kagviz/app/index.html>, driven headless
against the real tree — 407 sessions, kai/kubs0/cleo:

- Index ready in ~590 ms. No page errors, no failed requests.
- Filter to `cleo` → 116 rows. Sort by tools → the corpus's hardest session
  first: 2,777 calls, 45 failed, **1.62%** — the reference figure from
  `fmt.rs`'s own test.
- That session's page: 54d01h wall / 12h36m active / 53d12h idle, 390 phases
  ("mostly implementing"), 5,560 turns, 185 prompts, 232 files
  +24,958/−2,011 with 1,004 unseen, 209 spans and 208 collapsed breaks. Every
  figure matches the static report's.
- No horizontal page scroll at 1400px, 760px or 390px; light and dark both
  checked.
