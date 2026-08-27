# `web/` — the app over the facts

A static single-page app that reads the same three documents kagviz emits —
`sessions.json`, the facts and the events — and nothing else. No backend: it is
HTML, CSS and JS copied next to the data.

Sprint 011 built the skeleton, the contracts and the session browser; sprint
012 added the timeline's pan, zoom and click. The static report is unchanged
and stays; this does not replace it.

## Reading the code

| path                        | what                                                                                                                                                     |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/lib/contract/`         | the three documents in TypeScript, plus the decoders and the derived helpers. `conformance.spec.ts` is what makes it a contract — see below.             |
| `src/lib/data.ts`           | where the documents are fetched from, and what a failure says.                                                                                           |
| `src/lib/timeline.ts`       | the timeline's geometry — track, resolution, columns, bands, ticks, hit-testing. Pure, so all of it is testable. Supersedes 011's `strip.ts`.            |
| `src/lib/segment.ts`        | what a click resolves to, and the merge of the events with the facts' prompts and questions — the only place the app reads two documents for one answer. |
| `src/lib/browse.ts`         | sorting and filtering the index, likewise.                                                                                                               |
| `src/lib/format.ts`         | durations, counts, percentages — mirrors `src/fmt.rs` exactly.                                                                                           |
| `src/routes/+page.svelte`   | the session browser, `#/`.                                                                                                                               |
| `src/routes/s/[host]/[id]/` | the session page, `#/s/<host>/<id>` — and `?phase=3` / `?span=0&from=120&to=150`, the selection in the hash.                                             |
| `scripts/relativize.js`     | the post-build step that makes the shell mount-independent.                                                                                              |

## The timeline (sprint 012)

**One continuous zoom, not three levels.** The x-axis is **active seconds with
idle collapsed** — the coordinate the strip already used, and the reason it
reads at all — so a track is `Σ span.secs · pxPerSec + breaks · breakPx` and
`pxPerSec` is the whole zoom control. Forest, tree and leaf are three
neighbourhoods of one scale; the breadcrumb's buttons pick a `pxPerSec` rather
than switching modes, and "a column is a turn" falls out of the axis at deep
zoom instead of needing a fourth layout.

A **spawn** is the third kind of selection, opened from the delegated tier's
row rather than from the strip: phases cut the _parent's_ timeline, so a
spawn's events carry none and it is not on the strip at all. It is reconciled
against `delegation.spawns[k]`, and nothing from `user_involvement` is merged
into it — that is the parent's, and a subagent has no user.

**Above `bucket_secs` a column is whole facts buckets summed and the bar counts
`records`; below it there is nothing finer in the facts, so the column is
re-bucketed from the events and the bar counts turns and tool calls.** Those
are not the same number — `records` includes `system` and snapshot records,
which is why the contract says the events do not reproduce it — so the caption
names the resolution, the document _and_ the metric on every frame. **The fit
view is always the facts**, even once the events are here: it is the report's
strip, it is what the page draws before the big document lands, and a panel
that changes what its bars count the instant a fetch completes is what the
caption exists to prevent.

**A break grows with the columns, bounded by its share of the screen.** At leaf
zoom a 3px break between wide columns stops reading as a separator, and the one
thing a break says is that time was removed here. But letting it simply match
the column width reproduces the defect sprint 011 shipped — 208 breaks claiming
more than the viewport, every span squeezed to nothing. The three densities
were a proxy for the _share of the visible width_ the breaks take;
`breakWidth()` bounds that directly, with the densities as the floor. Read its
comment before touching it, and the test on 2/13/61/209 spans.

**Three things keep it fast on a million-pixel track.** One `<svg>` positioned
at the scroll offset with a `viewBox` in track coordinates, so only the visible
slice is ever in the DOM. One click handler with coordinate hit-testing, not a
listener per rect — and nothing focusable per column, which would be worse for
a keyboard than the `role="application"` container. And `ticks()` takes the
window rather than generating every tick and filtering, which was two million
objects a frame at leaf zoom.

## The three decisions sprint 011 made

**Hash routing.** copyparty serves files, not SPA fallbacks: a GET for
`/kagviz/app/s/kai/<id>` is a 404, not `index.html`. Putting the route in the
fragment means the server never sees it, so one `index.html` is the whole app.
It buys a second thing: the _document_ URL never changes as you navigate, so
`fetch('../sessions.json')` resolves the same way on every page.

**`derived/app/`, on the data's own origin.** No CORS and no k-homelab manifest
change — copyparty already serves `derived/` at `/kagviz/`. The tree stays
regenerable, which is the rule for everything under `derived/`; `just
web-deploy` rebuilds it, and `derive`/`index` never write there. The static
`index.html` links the app **only when it is actually deployed**, so the reader
can tell "not deployed" from "broken".

**Mount-independent, via a post-build step.** SvelteKit's `paths.relative`
rewrites asset URLs on a page it prerenders, because it knows how deep that
page sits. The hash-routing shell is generated as a _fallback_ — a page with no
path of its own — so kit leaves its `<link>` and `import()` URLs absolute and
only makes the runtime base relative. Mounted at `/kagviz/app/` those resolve
against the server root and the app never boots. Hash routing is exactly what
makes the fix safe: the shell is always `index.html` at the deployment root, so
`./_app/…` is right wherever the directory is copied. `scripts/relativize.js`
does the rewrite as `postbuild`, and **throws** if it finds nothing to rewrite
or anything left absolute — a kit release that changes this output fails the
build rather than shipping a page that cannot start.

## The host is replaceable, and that is deliberate

copyparty is a stop-gap. Nothing here depends on it: every mention of it in
this tree is a comment or a doc, and the app finds its data by resolving `../`
against `document.baseURI` — so it works under any HTTP mount, at any depth,
with no base path configured anywhere. Verified 2026-08-26 by serving the
deployed tree three directories deeper than production and driving it: 413
sessions, session page, zero errors.

**The rule that keeps it that way: the app must never need anything a dumb
static file server cannot do.** No SPA fallback (hence hash routing), no
rewrites, no directory-index behaviour, no range requests, no server-side
anything. Every document it reads is a file `kagviz derive` already wrote.

The floor is _a_ static HTTP server, though — not `file://`. The shell boots
through ES module `import()` and browsers refuse those on `file://`.

Since sprint 014 that property has a standing exercise rather than a one-off
verification. `just demo` deploys this bundle into a curated tree and serves it
with `python3 -m http.server` on a LAN address, so a second server and a second
origin get driven every time someone shows kagviz to anybody — including the
`file://` floor, which is why the demo serves rather than opening the directory.
See `docs/collection.md`.

Moving hosts is therefore a copy and a URL, and that option is worth more than
any single host is. Do not spend it.

## The conformance test

`src/lib/contract/conformance.spec.ts` reads `../tests/golden/fixture-0001.{facts,events,sessions}.json`
— the bytes the Rust binary actually emits, checked in and regenerated with
`KAGVIZ_UPDATE_GOLDEN=1` — puts them through the decoders, and asserts the
invariants `docs/facts-contract.md` states: the events sum to the facts per
phase and per spawn, `<unknown>` failures are carved out of both, `by_tool`
sums back to the totals, optional fields are absent rather than `null`, an
unknown field is ignored, and a document the app cannot read throws with the
path that failed.

It runs inside `just check` and CI, which is the point: **the app is the
contract's second consumer in the gate**. A facts change that breaks the
front-end fails the build on the Rust side the day it lands.

## Working on it

```sh
just web-check     # lint, svelte-check, build, vitest — what CI runs
just web-dev       # the dev server
just web-deploy    # build and install at derived/app/ on the served tree
```

There is no derived tree beside the dev server, so point the app at a served
one:

```sh
VITE_KAGVIZ_DERIVED=https://kai.encke-wahoo.ts.net:8027/kagviz/ just web-dev
```

## What is deliberately not here

- **A transcript viewer.** The segment panel shows what happened — turns,
  calls, durations, sizes, files — not what was said inside them. The events
  document says the same where it lists what it does not carry.
- **Zoom and pan in the hash.** Only the _selection_ is deep-linked; arriving
  with one frames it. A link that pinned a scroll offset would break the moment
  the window was a different width.
- **A combined active time.** A subagent runs while the session waits on it, so
  those seconds overlap rather than add. `contract/derived.ts` has a test
  asserting no such helper exists.
- **Any number the facts do not carry.** The renderer reads the facts, never
  the transcript — and so does this. A value the app wants and the facts do not
  have is a change to the facts.
