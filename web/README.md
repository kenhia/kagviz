# `web/` — the app over the facts

A static single-page app that reads the same three documents kagviz emits —
`sessions.json`, the facts, and (from sprint 012) the events — and nothing
else. No backend: it is HTML, CSS and JS copied next to the data.

Shipped in sprint 011 (part 1). The static report is unchanged and stays; this
does not replace it.

## Reading the code

| path                        | what                                                                                                                                         |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/lib/contract/`         | the three documents in TypeScript, plus the decoders and the derived helpers. `conformance.spec.ts` is what makes it a contract — see below. |
| `src/lib/data.ts`           | where the documents are fetched from, and what a failure says.                                                                               |
| `src/lib/strip.ts`          | the time strip's geometry, pure so it can be tested.                                                                                         |
| `src/lib/browse.ts`         | sorting and filtering the index, likewise.                                                                                                   |
| `src/lib/format.ts`         | durations, counts, percentages — mirrors `src/fmt.rs` exactly.                                                                               |
| `src/routes/+page.svelte`   | the session browser, `#/`.                                                                                                                   |
| `src/routes/s/[host]/[id]/` | the session page, `#/s/<host>/<id>`.                                                                                                         |
| `scripts/relativize.js`     | the post-build step that makes the shell mount-independent.                                                                                  |

## The three decisions this sprint made

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

- **Pan, zoom, and the click into a timeline segment.** Sprint 012 (#1591,
  #1639). The strip is drawn once, at the session's own `bucket_secs`; finer
  buckets come from the events document, which is why `MAX_BUCKETS` stayed at
  240 when that document was designed.
- **A combined active time.** A subagent runs while the session waits on it, so
  those seconds overlap rather than add. `contract/derived.ts` has a test
  asserting no such helper exists.
- **Any number the facts do not carry.** The renderer reads the facts, never
  the transcript — and so does this. A value the app wants and the facts do not
  have is a change to the facts.
