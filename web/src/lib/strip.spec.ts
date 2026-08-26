import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { decodeFacts } from './contract/facts.js';
import { parse } from './contract/decode.js';
import { NARROW_BREAKS_MIN, density, layout } from './strip.js';

const facts = decodeFacts(
	parse(
		readFileSync(
			fileURLToPath(new URL('../../../tests/golden/fixture-0001.facts.json', import.meta.url)),
			'utf8'
		),
		'facts'
	)
);

describe('the strip', () => {
	it('draws a band for every phase of every span, and counts the rest', () => {
		const strip = layout(facts);
		expect(strip.spans).toHaveLength(facts.activity.spans.length);
		const bands = strip.spans.reduce((n, s) => n + s.bands.length, 0);
		expect(bands + strip.phasesWithoutBand).toBe(facts.phases.length);
	});

	it('tiles each span with its phases, edge to edge', () => {
		for (const [i, span] of layout(facts).spans.entries()) {
			const buckets = facts.activity.spans[i].buckets.length;
			expect(span.bands[0].from).toBeCloseTo(0, 6);
			expect(span.bands[span.bands.length - 1].to).toBeLessThanOrEqual(buckets);
			for (let b = 1; b < span.bands.length; b++) {
				expect(span.bands[b].from).toBeCloseTo(span.bands[b - 1].to, 6);
			}
		}
	});

	it('collapses idle to a break rather than a stretch of nothing', () => {
		const strip = layout(facts);
		expect(strip.spans[0].idleBefore).toBe(0);
		expect(strip.spans.slice(1).every((s) => s.idleBefore > 0)).toBe(true);
		// Idle occupies no buckets at all — that is what lets it collapse.
		expect(strip.spans.every((s) => s.columns.length > 0)).toBe(true);
	});

	it('places a mark on the bucket that holds each involvement moment', () => {
		const marks = layout(facts)
			.spans.flatMap((s) => s.columns)
			.filter((c) => c.mark);
		expect(marks.length).toBeGreaterThan(0);
		expect(marks.some((c) => c.mark?.kind === 'question')).toBe(true);
		expect(marks.some((c) => c.mark?.kind === 'prompt')).toBe(true);
	});

	/**
	 * The one that shipped broken. Breaks are a fixed width and spans have no
	 * width of their own, so on the corpus's 209-span session 208 breaks
	 * claimed more than the whole strip and every span was squeezed to
	 * nothing: the panel whose job is collapsing idle rendered only idle. The
	 * report solved it with three densities; this holds the app to the same
	 * thresholds.
	 */
	it('narrows the breaks before they can crowd out the work', () => {
		expect(density(2)).toBe('roomy');
		expect(density(12)).toBe('roomy');
		expect(density(13)).toBe('dense');
		expect(density(NARROW_BREAKS_MIN)).toBe('dense');
		expect(density(NARROW_BREAKS_MIN + 1)).toBe('packed');
		expect(density(209)).toBe('packed');
	});

	it('labels a break only where there is room for the words', () => {
		expect(layout(facts).labelBreaks).toBe(true);
	});
});
