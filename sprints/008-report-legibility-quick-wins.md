# Sprint 008 — report legibility quick wins

korg:1621 · covers #1590 (the tool failure rate beside the failure count),
#1614 (a zoom-in checkbox for dense strips — CSS only, the interim for #1591)

## Goal

Two things Ken pointed at on the sprint 002 report, both presentation, both
small: a failure count with no denominator beside it, and a time strip that
turns into a wall of 2px bars on any long session. The cheap sprint between
007 (collection) and 009 (the facts learn detail). **No fact moved** — the
JSON contract is byte-identical, and the sweep below proves it rather than
asserts it.

## #1590 — the rate beside the count

`2777` / `45 failed` now reads `45 failed · 1.62%`, and the terminal `show`
says `2777 calls, 45 failed (1.62%)`.

- **One place owns the arithmetic.** `Summary::tool_failure_rate()` sits
  beside `combined_tool_calls` — a method, not a field, for the same reason:
  a quotient of two numbers the facts already carry is not a separate fact.
  `fmt::percent` is the shared formatter, so the two presentation layers
  cannot drift. The contract doc gained a paragraph saying which two choices
  a consumer computing its own rate has to make to agree with the report.
- **Denominator: the calls.** Ken's own reading of hv-simulator was 1.59%
  (`45 / (2777 + 45)`); a failed call is a call, already counted once, so it
  is `45 / 2777`. Decided in the WI; implemented as decided.
- **Two decimals, not one.** The WI says "one decimal place" and in the same
  breath "1.6% loses too much, 1.62% is fine". The example wins over the
  word: `1.62%` is what Ken's own mock-up shows. One character in
  `fmt::percent` if that was the wrong call.
- **`<unknown>` failures leave the numerator.** Their calls are not in
  `tool_calls`, so they are not in the denominator either; counting them lets
  two unjoined failures beside one succeeded call read `200.00%`. When every
  failure is unknown there is no rate — the count and the Tools card's
  "could not be joined" note stand alone. Zero of 402 corpus sessions hit
  this (re-measured); the test pins it anyway.
- The zero case stays bare `none failed`.

## #1614 — zoom in, without a script

A checkbox above the strip. Checked, every column takes a fixed readable
width and the strip scrolls sideways. `render::tests::the_report_is_self_contained`
did not change and still passes: the mechanism is `<input type="checkbox">`
and `:checked ~ .strip` sibling rules, nothing else.

- **What the zoom does, exactly.** Spans stop shrinking and take their
  content width; columns get `flex-basis:12px` and stop shrinking; both keep
  their `flex-grow`, so a strip with room to spare still fills it. The zoom
  therefore only ever *widens* — it can never make a legible strip worse.
  With width no longer scarce, the packed 3px breaks get their 6px and dashed
  edges back.
- **12px is the reference.** Measured off Ken's goldilocks snip (img-639 on
  #1591): 108 columns over 1244px, ~11.5px each.
- **When it is offered: `ZOOM_MIN_BUCKETS = 120`.** At the full 1480px layout
  the strip is ~1,400px, which holds ~116 columns at 12px; below that the
  box would change nothing at full width, and a control that does nothing is
  worse than one that arrives a little late on a half-width window. The
  goldilocks snip is 118 columns filling exactly that width. Across the
  corpus the box appears on 252 of 402 sessions — `MAX_BUCKETS` is 240, so
  anything past an hour or two saturates the strip. One constant if Ken wants
  it rarer.
- **What the zoom exposed.** Under a zoomed one-column span the axis showed
  `0€` — the first glyph and a half of `06-13 00:47:18` — and the unzoomed
  hv-simulator strip has two hundred of those in a row (it is in img-638,
  Ken's own snip). The axis text is now gated on the span's rendered width by
  a container query, the way band labels have been since 002: nothing,
  then the duration, then both. Fixed height so an empty axis still lines up.
  Same mechanism as `BAND_LABEL_MIN_PX`; there is no Rust-side number to
  mirror, so it is CSS with a comment.
- **Checked visually, not just by test.** Playwright's chromium binary is on
  kai (`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome`) even though
  the `playwright` module is not; `--headless=new --screenshot` renders the
  report, and `sed`-ing ` checked` onto the input renders the zoomed state.
  Looked at: hv-simulator at 1480 and 760px, checked and not; the short
  reference session likewise.

## What did not change

- The facts JSON. `kagviz show --json` over every kai session in the derived
  corpus, compared byte-for-byte against the facts `derive` wrote from main
  (44c1702): 191 of 194 identical, and the 3 that differ are sessions whose
  transcript is newer than its facts file — resumed since the nightly ran,
  not changed by this branch.
- `NARROW_BREAKS_MIN` and the density classes — the comment on that constant
  still says "the fix is zooming, not shaving pixels", and now there is one.
- Anything under `collect/` or `derive`: reports regenerate on the next
  nightly because the kagviz stamp changes, which is the designed path.

## Follow-ups

- #1591 stays open: this is the interim. The real pan/zoom is front-end v1
  (#1619), after 009 gives it events to zoom into.
- If the box is too eager, `ZOOM_MIN_BUCKETS` is the dial; if `1.62%` should
  have been `1.6%`, `fmt::percent` is the character.
