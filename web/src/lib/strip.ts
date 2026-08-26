/**
 * The time strip's geometry — pure, so it can be tested without a DOM.
 *
 * Drawn from `activity`: idle occupies no buckets at all, which is what lets
 * a break be collapsed to a fixed width instead of a proportional one. Bands
 * come from `phases` and tile their span exactly, because the phases of one
 * span sum to that span's `secs`. Marks come from `user_involvement`, mapped
 * onto the bucket that holds them.
 *
 * `bucket_secs` is a property of the **session**, not the renderer — so this
 * draws the series at the resolution the facts chose and never re-buckets.
 * Re-bucketing past that resolution is the events document's job, and part 2's.
 */

import type { Facts, Phase, PhaseKind } from './contract/facts.js';
import { clock, count, duration, epoch } from './format.js';

/** Bars are given a floor so a bucket with a record is never invisible. */
const MIN_BAR_PCT = 6;

/** Above this many stretches of work there is no room to label the breaks. */
export const LABELLED_BREAKS_MAX = 12;

/**
 * Above this many, the breaks narrow so the work columns keep the width.
 *
 * The same constants the report uses, for the same measured reason: on the
 * corpus's 209-span session, 208 fixed-width breaks claimed more than the
 * whole strip and the spans — which have no width of their own — were
 * squeezed to nothing. The panel whose job is collapsing idle was rendering
 * *only* idle. A break has to be visible, not proportional (its duration is
 * deliberately not to scale), so past this count it narrows and gives the
 * width back.
 */
export const NARROW_BREAKS_MIN = 60;

export type Density = 'roomy' | 'dense' | 'packed';

export function density(spans: number): Density {
	if (spans <= LABELLED_BREAKS_MAX) return 'roomy';
	if (spans <= NARROW_BREAKS_MIN) return 'dense';
	return 'packed';
}

export interface Column {
	/** Bar height as a percentage of the track. `0` for an empty bucket. */
	pct: number;
	failed: boolean;
	mark?: Mark;
	tip: string;
}

export interface Mark {
	kind: 'prompt' | 'question';
	tip: string;
}

export interface Band {
	kind: PhaseKind;
	/** Start and end in bucket units within the span. */
	from: number;
	to: number;
	tip: string;
	label: string;
	/** The model-written label, when there is one. Marked wherever shown. */
	written?: string;
}

export interface SpanLayout {
	/** The idle gap before this span, in seconds. `0` for the first. */
	idleBefore: number;
	columns: Column[];
	bands: Band[];
	started: string;
	secs: number;
}

export interface StripLayout {
	spans: SpanLayout[];
	bucketSecs: number;
	/** Phases too short to draw a band for — counted, never dropped. */
	phasesWithoutBand: number;
	density: Density;
	/** Whether a break has room for its duration in words. */
	labelBreaks: boolean;
}

export function layout(f: Facts): StripLayout {
	const peak = Math.max(1, ...f.activity.spans.flatMap((s) => s.buckets.map((b) => b.records)));
	const width = Math.max(1, f.activity.bucket_secs);
	const marks = marksByBucket(f, width);

	let drawn = 0;
	const spans: SpanLayout[] = f.activity.spans.map((span, si) => {
		const start = epoch(span.started) ?? 0;
		const columns = span.buckets.map((b, bi) => {
			const at = new Date(start + bi * width * 1000).toISOString();
			const mark = marks.get(`${si}:${bi}`);
			return {
				pct: b.records === 0 ? 0 : Math.min(100, MIN_BAR_PCT + Math.ceil((b.records * 94) / peak)),
				failed: b.tool_failures > 0,
				mark,
				tip: columnTip(at, b, mark)
			};
		});
		const bands: Band[] = [];
		for (const [i, p] of f.phases.entries()) {
			if (p.span !== si) continue;
			const from = offset(p.started, start, width);
			const to = offset(p.ended, start, width);
			if (to <= from) continue;
			drawn += 1;
			bands.push({
				kind: p.kind,
				from,
				to: Math.min(to, span.buckets.length),
				tip: phaseTip(p),
				label: p.kind,
				written: f.labels?.phases.find((l) => l.phase === i)?.label
			});
		}
		return {
			idleBefore: span.idle_before_secs,
			columns,
			bands,
			started: span.started,
			secs: span.secs
		};
	});

	return {
		spans,
		bucketSecs: width,
		phasesWithoutBand: f.phases.length - drawn,
		density: density(spans.length),
		labelBreaks: spans.length <= LABELLED_BREAKS_MAX
	};
}

function offset(iso: string, spanStart: number, width: number): number {
	const t = epoch(iso);
	if (t === undefined) return 0;
	return Math.max(0, (t - spanStart) / 1000 / width);
}

function columnTip(
	at: string,
	b: { records: number; tool_calls: number; tool_failures: number; output_tokens: number },
	mark: Mark | undefined
): string {
	let tip = `${clock(at)} · ${b.records} record(s)`;
	if (b.tool_calls > 0) tip += `, ${b.tool_calls} tool call(s)`;
	if (b.tool_failures > 0) tip += `, ${b.tool_failures} failed`;
	if (b.output_tokens > 0) tip += `, ${count(b.output_tokens)} out`;
	if (mark) tip += ` — ${mark.tip}`;
	return tip;
}

function phaseTip(p: Phase): string {
	let tip = `${p.kind} · ${clock(p.started)} · ${duration(p.secs)} · ${p.tool_calls} tool call(s)`;
	if (p.tool_failures > 0) tip += `, ${p.tool_failures} failed`;
	if (p.opened_by) tip += ` — ${p.opened_by}`;
	return tip;
}

/**
 * Each involvement moment on the bucket that holds it. A moment without a
 * timestamp is simply not placed — inventing one would be the whole project's
 * failure in miniature.
 */
function marksByBucket(f: Facts, width: number): Map<string, Mark> {
	const out = new Map<string, Mark>();
	for (const item of f.user_involvement) {
		const at = epoch(item.at);
		if (at === undefined) continue;
		const si = f.activity.spans.findIndex((sp) => {
			const from = epoch(sp.started);
			const to = epoch(sp.ended);
			return from !== undefined && to !== undefined && at >= from && at <= to;
		});
		if (si < 0) continue;
		const span = f.activity.spans[si];
		const from = epoch(span.started) ?? 0;
		const bi = Math.min(
			Math.floor((at - from) / 1000 / width),
			Math.max(0, span.buckets.length - 1)
		);
		const tip =
			item.kind === 'prompt'
				? item.preview || 'user pasted an attachment'
				: `asked: ${item.question}`;
		const key = `${si}:${bi}`;
		const existing = out.get(key);
		if (existing) existing.tip += ` / ${tip}`;
		else out.set(key, { kind: item.kind, tip });
	}
	return out;
}
