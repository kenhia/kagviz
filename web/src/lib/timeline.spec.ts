/**
 * The timeline's geometry, over the repo's own golden facts and events.
 *
 * Carries forward sprint 011's `strip.spec.ts` — the 209-span case in
 * particular, which is the one that shipped broken — and adds what the axis
 * brought with it: that a column is exact when it comes from the facts, that
 * it comes from the events only where the facts have nothing finer, and that
 * the two are never quietly swapped.
 */

import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { decodeFacts } from './contract/facts.js';
import { decodeEvents } from './contract/events.js';
import { parse } from './contract/decode.js';
import {
	BREAK_PX,
	BREAK_PX_MAX,
	BREAK_SHARE_MAX,
	MAX_PX_PER_SEC,
	MIN_COL_PX,
	NARROW_BREAKS_MIN,
	bands,
	breakWidth,
	columns,
	density,
	fitPxPerSec,
	marks,
	resolution,
	ticks,
	track,
	visible
} from './timeline.js';

const GOLDEN = fileURLToPath(new URL('../../../tests/golden/', import.meta.url));
const facts = decodeFacts(parse(readFileSync(`${GOLDEN}fixture-0001.facts.json`, 'utf8'), 'facts'));
const events = decodeEvents(
	parse(readFileSync(`${GOLDEN}fixture-0001.events.json`, 'utf8'), 'events')
);

const VIEWPORT = 1200;
const fit = fitPxPerSec(facts, VIEWPORT);

describe('the track', () => {
	it('is active seconds with idle collapsed, not wall time', () => {
		const t = track(facts, fit);
		expect(t.activeSecs).toBe(facts.active_secs);
		// Wall time is longer, and none of the difference is on the axis.
		expect(facts.wall_secs).toBeGreaterThan(t.activeSecs);
		const breaks = (t.spans.length - 1) * t.breakPx;
		expect(t.width).toBeCloseTo(t.activeSecs * fit + breaks, 6);
	});

	it('lays the spans out in order with a break between each pair', () => {
		const t = track(facts, fit);
		expect(t.spans[0].x).toBe(0);
		expect(t.spans[0].idleBefore).toBe(0);
		for (let i = 1; i < t.spans.length; i++) {
			expect(t.spans[i].x).toBeCloseTo(t.spans[i - 1].x + t.spans[i - 1].w + t.breakPx, 6);
			expect(t.spans[i].idleBefore).toBeGreaterThan(0);
		}
	});

	it('fits the whole session in the viewport it was given', () => {
		expect(track(facts, fit).width).toBeLessThanOrEqual(VIEWPORT + 1e-6);
	});

	it('never lets a span vanish, however short', () => {
		for (const s of track(facts, fitPxPerSec(facts, 40)).spans) {
			expect(s.w).toBeGreaterThanOrEqual(MIN_COL_PX);
		}
	});

	/**
	 * The one that shipped broken in 011. Breaks are a fixed width and spans
	 * have no width of their own, so on the corpus's 209-span session 208
	 * breaks claimed more than the whole strip and every span was squeezed to
	 * nothing: the panel whose job is collapsing idle rendered only idle. The
	 * report solved it with three densities; this holds the timeline to the
	 * same thresholds, at every zoom rather than only at fit.
	 */
	it('narrows the breaks before they can crowd out the work', () => {
		expect(density(2)).toBe('roomy');
		expect(density(12)).toBe('roomy');
		expect(density(13)).toBe('dense');
		expect(density(NARROW_BREAKS_MIN)).toBe('dense');
		expect(density(NARROW_BREAKS_MIN + 1)).toBe('packed');
		expect(density(209)).toBe('packed');
		expect(BREAK_PX.packed).toBeLessThan(BREAK_PX.dense);
		expect(BREAK_PX.dense).toBeLessThan(BREAK_PX.roomy);
	});

	it('keeps a break the same width at every zoom', () => {
		expect(track(facts, fit).breakPx).toBe(track(facts, fit * 40).breakPx);
	});
});

describe('the resolution', () => {
	const width = facts.activity.bucket_secs;

	it('reads the facts at or above the session bucket, and sums whole buckets', () => {
		for (const px of [fit, fit * 2, MIN_COL_PX / width]) {
			const r = resolution(px, width);
			if (r.source !== 'facts') continue;
			expect(r.buckets).toBeGreaterThanOrEqual(1);
			expect(Number.isInteger(r.buckets)).toBe(true);
			expect(r.secs).toBe(width * r.buckets);
		}
	});

	it('reaches for the events only below the bucket the facts chose', () => {
		const deep = resolution(24, width);
		expect(deep.source).toBe('events');
		expect(deep.secs).toBeLessThan(width);
		expect(resolution(MIN_COL_PX / width, width)).toMatchObject({ source: 'facts', buckets: 1 });
	});

	it('keeps every column at least MIN_COL_PX wide', () => {
		for (const px of [0.001, 0.05, 0.4, 1, 4, 24]) {
			expect(resolution(px, width).secs * px).toBeGreaterThanOrEqual(MIN_COL_PX - 1e-9);
		}
	});
});

describe('the columns', () => {
	it('sum the facts exactly when they come from the facts', () => {
		const r = resolution(fit, facts.activity.bucket_secs);
		expect(r.source).toBe('facts');
		const cols = columns(facts, undefined, track(facts, fit), r)!;
		expect(cols.metric).toBe('records');
		const drawn = cols.items.reduce((n, c) => n + c.value, 0);
		const held = facts.activity.spans.flatMap((s) => s.buckets).reduce((n, b) => n + b.records, 0);
		expect(drawn).toBe(held);
	});

	it('count turns and tool calls when they come from the events, and say so', () => {
		const t = track(facts, 24);
		const r = resolution(24, facts.activity.bucket_secs);
		const cols = columns(facts, events, t, r)!;
		expect(cols.source).toBe('events');
		expect(cols.metric).toBe('events');
		const drawn = cols.items.reduce((n, c) => n + c.value, 0);
		expect(drawn + cols.unplaced).toBe(events.events.length);
	});

	it('has nothing to draw at a fine resolution until the events arrive', () => {
		const r = resolution(24, facts.activity.bucket_secs);
		expect(columns(facts, undefined, track(facts, 24), r)).toBeUndefined();
	});

	it('places every column inside its own span', () => {
		const t = track(facts, fit);
		const cols = columns(facts, undefined, t, resolution(fit, facts.activity.bucket_secs))!;
		for (const c of cols.items) {
			const s = t.spans[c.span];
			expect(c.x).toBeGreaterThanOrEqual(s.x - 1e-6);
			expect(c.x + c.w).toBeLessThanOrEqual(s.x + s.w + 1e-6);
		}
	});
});

describe('the bands and the marks', () => {
	it('draw a band per phase, or count the ones too narrow to draw', () => {
		const b = bands(facts, track(facts, fit));
		expect(b.items.length + b.tooNarrow).toBe(facts.phases.length);
	});

	it('tile each span edge to edge, in order', () => {
		const t = track(facts, fit);
		const b = bands(facts, t);
		for (const s of t.spans) {
			const mine = b.items.filter((i) => facts.phases[i.phase].span === s.index);
			if (mine.length === 0) continue;
			expect(mine[0].x).toBeCloseTo(s.x, 6);
			for (let i = 1; i < mine.length; i++) {
				expect(mine[i].x).toBeCloseTo(mine[i - 1].x + mine[i - 1].w, 6);
			}
			const last = mine[mine.length - 1];
			expect(last.x + last.w).toBeLessThanOrEqual(s.x + s.w + 1e-6);
		}
	});

	it('reveals phases the fit view was too coarse to draw', () => {
		const coarse = bands(facts, track(facts, fit)).tooNarrow;
		expect(bands(facts, track(facts, fit * 200)).tooNarrow).toBeLessThanOrEqual(coarse);
	});

	it('places a mark where each involvement moment happened', () => {
		const m = marks(facts, track(facts, fit));
		const placeable = facts.user_involvement.filter((i) => i.at !== undefined).length;
		expect(m).toHaveLength(placeable);
		expect(m.some((x) => x.kind === 'prompt')).toBe(true);
		expect(m.some((x) => x.kind === 'question')).toBe(true);
	});
});

describe('the axis', () => {
	it('never draws a tick across a break — that is a time that did not happen', () => {
		const t = track(facts, fit * 20);
		for (const tick of ticks(t, 0, t.width)) {
			expect(t.spans.some((s) => tick.x >= s.x - 1e-6 && tick.x <= s.x + s.w + 1e-6)).toBe(true);
		}
	});

	it('draws only what the window asked for, not the whole track', () => {
		// Generating every tick of every span and filtering afterwards is two
		// million objects a frame at leaf zoom on a twelve-hour session.
		const t = track(facts, MAX_PX_PER_SEC);
		const whole = ticks(t, 0, t.width);
		const window = ticks(t, 0, 600);
		expect(window.length).toBeLessThan(whole.length);
		for (const k of window) expect(k.x).toBeLessThanOrEqual(600);
	});

	/**
	 * Found by looking: at one second per column the axis read `19:19` eight
	 * times across, which is not a clock, it is noise where a clock should be.
	 */
	it('says the seconds once a column is finer than a minute', () => {
		const deep = ticks(track(facts, MAX_PX_PER_SEC), 0, 4000);
		expect(deep.length).toBeGreaterThan(0);
		for (const k of deep) expect(k.label).toMatch(/^\d\d:\d\d:\d\d$/);
		const coarse = ticks(track(facts, fit), 0, 1e7, 96);
		for (const k of coarse) expect(k.label).toMatch(/^\d\d:\d\d$/);
	});

	it('draws no axis at all rather than one nobody can read', () => {
		const t = track(facts, fit);
		expect(ticks(t, 0, t.width, 1e9)).toHaveLength(0);
	});
});

/**
 * The other half of what "looking at it" turned up: a 3px break between 24px
 * columns stops reading as a separator, and a break is the one mark that says
 * time was removed here — 53 minutes, on the corpus's hardest session.
 */
describe('a break', () => {
	const VIEW = 1400;
	const at = (px: number) =>
		breakWidth(facts, px, VIEW, resolution(px, facts.activity.bucket_secs, true).secs * px);

	it('grows with the columns as the zoom goes in', () => {
		// On the golden's handful of spans the fit break is already the roomy
		// width, so there is nothing to grow into; the crowded case is the one
		// that starts at 3px and has to become legible.
		const many = crowded(209);
		const px = fitPxPerSec(many, VIEW);
		const wide = (p: number) =>
			breakWidth(many, p, VIEW, resolution(p, many.activity.bucket_secs, true).secs * p);
		expect(wide(px)).toBeCloseTo(BREAK_PX.packed, 1);
		expect(wide(MAX_PX_PER_SEC)).toBeGreaterThan(wide(px));
		expect(at(MAX_PX_PER_SEC)).toBeGreaterThanOrEqual(at(fit));
	});

	it('is bounded, however far the zoom goes — a gap is not to scale', () => {
		expect(at(MAX_PX_PER_SEC)).toBeLessThanOrEqual(BREAK_PX_MAX);
		expect(breakWidth(facts, 1e6, VIEW, 1e6)).toBe(BREAK_PX_MAX);
	});

	it("never falls below the report's own density floor", () => {
		expect(at(fit)).toBeGreaterThanOrEqual(BREAK_PX[density(facts.activity.spans.length)]);
	});

	/**
	 * The regression this rule exists to prevent, and it was shipped once:
	 * letting a break grow to a column's width means 208 of them claim more
	 * than the whole viewport at fit, every span is squeezed to nothing, and
	 * the panel whose job is collapsing idle renders only idle.
	 */
	it('never lets the gaps claim the viewport, however many there are', () => {
		for (const spans of [2, 13, 61, 209]) {
			const many = crowded(spans);
			const px = fitPxPerSec(many, VIEW);
			const w = breakWidth(
				many,
				px,
				VIEW,
				resolution(px, many.activity.bucket_secs, false).secs * px
			);
			expect((spans - 1) * w).toBeLessThanOrEqual(VIEW * BREAK_SHARE_MAX + 1e-6);
		}
	});

	it('still fits the whole session in the viewport it was given', () => {
		for (const w of [400, 900, 1400]) {
			const px = fitPxPerSec(facts, w);
			const r = resolution(px, facts.activity.bucket_secs, false);
			const t = track(facts, px, breakWidth(facts, px, w, r.secs * px));
			expect(t.width).toBeLessThanOrEqual(w + 1e-6);
		}
	});

	/** The golden has few spans; the case that broke has 209. */
	function crowded(spans: number) {
		const one = facts.activity.spans[0];
		return {
			...facts,
			activity: {
				...facts.activity,
				spans: Array.from({ length: spans }, (_, i) => ({
					...one,
					idle_before_secs: i === 0 ? 0 : 3600
				}))
			}
		};
	}
});

describe('virtualising', () => {
	it('returns exactly what the window overlaps', () => {
		const t = track(facts, fit);
		const cols = columns(facts, undefined, t, resolution(fit, facts.activity.bucket_secs))!;
		const some = visible(cols.items, 100, 200);
		expect(some.length).toBeGreaterThan(0);
		expect(some.length).toBeLessThan(cols.items.length);
		for (const c of some) expect(c.x <= 200 && c.x + c.w >= 100).toBe(true);
		expect(visible(cols.items, 0, t.width)).toHaveLength(cols.items.length);
	});
});
