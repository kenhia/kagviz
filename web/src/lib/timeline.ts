/**
 * The timeline's geometry — pure, so it can be tested without a DOM.
 *
 * Supersedes sprint 011's `strip.ts`, which drew the series once at the
 * session's own `bucket_secs` and could not do anything else. This carries
 * that layout forward as the *fit* zoom and adds the axis it was missing.
 *
 * ## The coordinate system
 *
 * The x-axis is **active seconds, with idle collapsed** — the same choice the
 * strip made and the reason it works: a span is proportional to its length, an
 * idle break is a fixed-width mark between two spans rather than a
 * proportional stretch of nothing. So a track is
 *
 *     width = Σ span.secs · pxPerSec  +  breaks · breakPx
 *
 * and `pxPerSec` is the whole zoom control. Panning is a scroll offset in px
 * on that track; zooming recomputes it. There are no "levels" — forest, tree
 * and leaf are three neighbourhoods of one continuous scale, and the
 * breadcrumb's buttons are shortcuts that pick a `pxPerSec`, not modes.
 *
 * ## Where a column's numbers come from, and why that changes with zoom
 *
 * `bucket_secs` is a property of the **session**: the facts resolve the series
 * at exactly that width and no finer (`MAX_BUCKETS = 240`). So:
 *
 * - **At or above `bucket_secs`** a column is a whole number of facts buckets
 *   summed — exact, and identical to what the static report draws. Its bar
 *   counts `records`.
 * - **Below `bucket_secs`** the facts cannot be split, and the column is
 *   re-bucketed from the **events document** instead. Its bar counts turns and
 *   tool calls, because that is what the events carry.
 *
 * The two are not the same number and must never be drawn as if they were:
 * `records` counts every timestamped record, `system` and snapshot records
 * included, and the contract says so where it says the events do not reproduce
 * it. `Columns.metric` is what the caption and the legend read off, so the
 * page always names which one is on screen.
 */

import type { Bucket, Facts, PhaseKind } from './contract/facts.js';
import type { EventsDocument } from './contract/events.js';
import { isTool } from './contract/events.js';
import { clock, count, duration, epoch } from './format.js';

/** Bars are given a floor so a column with a record is never invisible. */
const MIN_BAR_PCT = 6;

/** Narrower than this and a column is not a mark, it is an artefact. */
export const MIN_COL_PX = 3;

/**
 * The width a column has to keep to be worth *splitting* one into.
 *
 * `MIN_COL_PX` is a floor — below it a column is not readable at all.
 * This is a different question: given room, how fine should the series get?
 * 12px is Ken's own reference density (img-639 on #1591, against img-638's
 * wall of 2px bars), so the timeline subdivides a bucket only when the pieces
 * would still read at the density the zoom exists to reach.
 */
export const TARGET_COL_PX = 12;

/** Above this many stretches of work there is no room to label the breaks. */
export const LABELLED_BREAKS_MAX = 12;

/**
 * Above this many, the breaks narrow so the work columns keep the width.
 *
 * The same constants the report uses, for the same measured reason: on the
 * corpus's 209-span session, 208 fixed-width breaks claimed more than the
 * whole strip and the spans — which have no width of their own — were squeezed
 * to nothing. The panel whose job is collapsing idle was rendering *only*
 * idle. A break has to be visible, not proportional (its duration is
 * deliberately not to scale), so past this count it narrows and gives the
 * width back.
 *
 * A break keeps its width at every zoom rather than growing with the columns.
 * That is deliberate: the width is chosen from the *session's* span count, so
 * the fit view is the report's strip exactly, and panning cannot make a gap
 * appear to have changed length.
 */
export const NARROW_BREAKS_MIN = 60;

export type Density = 'roomy' | 'dense' | 'packed';

export function density(spans: number): Density {
	if (spans <= LABELLED_BREAKS_MAX) return 'roomy';
	if (spans <= NARROW_BREAKS_MIN) return 'dense';
	return 'packed';
}

export const BREAK_PX: Record<Density, number> = { roomy: 32, dense: 6, packed: 3 };

/** No break is drawn wider than this, however far in the zoom goes. */
export const BREAK_PX_MAX = 32;

/**
 * The most of the visible width the collapsed gaps may claim between them.
 *
 * Measured, not chosen: on the corpus's 209-span session the report's own
 * densities put the breaks at 47% of the strip, and that is the most idle
 * should ever take on a panel whose job is collapsing it.
 */
export const BREAK_SHARE_MAX = 0.45;

/**
 * How wide a collapsed gap is drawn at this zoom.
 *
 * Two forces, and each was found by shipping the other one alone.
 *
 * At **leaf** zoom a 3px break between wide columns stops reading as a
 * separator at all, and the one thing a break has to say is that time was
 * removed here — 53 minutes, between two columns sitting a few pixels apart.
 * So a break grows with the columns.
 *
 * At **fit** that rule alone reproduces the defect sprint 011 shipped: 208
 * breaks at a column's width claim more than the whole viewport, every span is
 * squeezed to nothing, and the panel whose job is collapsing idle renders only
 * idle. The report's three densities were the fix; what they are a proxy for
 * is the *share of the visible width* the breaks take, and that is bounded
 * here directly. How many breaks are on screen follows from the zoom alone —
 * active seconds visible, over the average span — so nothing here depends on
 * the layout it feeds, and `fitPxPerSec` converges in one extra pass.
 *
 * A break is still never *proportional*: an hour and a day collapse to the
 * same mark, which is the whole point of collapsing them.
 */
export function breakWidth(f: Facts, pxPerSec: number, viewport: number, colPx: number): number {
	const spans = f.activity.spans.length;
	const floor = BREAK_PX[density(spans)];
	if (spans < 2 || pxPerSec <= 0 || viewport <= 0) return floor;
	const active = f.activity.spans.reduce((n, s) => n + s.secs, 0);
	const avgSpan = active > 0 ? active / spans : 1;
	const onScreen = Math.min(spans - 1, Math.max(1, viewport / pxPerSec / avgSpan));
	const room = (viewport * BREAK_SHARE_MAX) / onScreen;
	return Math.min(BREAK_PX_MAX, Math.max(floor, Math.min(colPx, room)));
}

/** One second per 24px is the floor of useful zoom: below a turn there is nothing. */
export const MAX_PX_PER_SEC = 24;

export interface TrackSpan {
	index: number;
	started: string;
	startedMs: number;
	secs: number;
	/** The idle gap before this span, in seconds. `0` for the first. */
	idleBefore: number;
	/** Left edge on the track, in px. */
	x: number;
	/** Width on the track, in px. */
	w: number;
	/**
	 * Px per second *inside this span* — `pxPerSec`, except in a span so short
	 * it would otherwise be invisible, where the floor stretches it. A span of
	 * zero length still holds records, and dropping it would report a thing
	 * that happened as a thing that did not.
	 */
	scale: number;
}

export interface Track {
	spans: TrackSpan[];
	breakPx: number;
	density: Density;
	/** Whether a break has room for its duration in words. */
	labelBreaks: boolean;
	pxPerSec: number;
	/** Total track width in px, breaks included. */
	width: number;
	activeSecs: number;
}

export function track(f: Facts, pxPerSec: number, breakPx?: number): Track {
	const d = density(f.activity.spans.length);
	breakPx ??= BREAK_PX[d];
	const spans: TrackSpan[] = [];
	let x = 0;
	let activeSecs = 0;
	for (const [i, s] of f.activity.spans.entries()) {
		if (i > 0) x += breakPx;
		const raw = s.secs * pxPerSec;
		const w = Math.max(raw, MIN_COL_PX);
		spans.push({
			index: i,
			started: s.started,
			startedMs: epoch(s.started) ?? 0,
			secs: s.secs,
			idleBefore: s.idle_before_secs,
			x,
			w,
			scale: s.secs > 0 ? w / s.secs : 0
		});
		x += w;
		activeSecs += s.secs;
	}
	return {
		spans,
		breakPx,
		density: d,
		labelBreaks: f.activity.spans.length <= LABELLED_BREAKS_MAX,
		pxPerSec,
		width: x,
		activeSecs
	};
}

/**
 * The zoom at which the whole session fits `viewport` px.
 *
 * This is the minimum: there is nothing to see zoomed out past the session,
 * and letting the track shrink inside its own frame reads as a bug.
 *
 * Two passes, because a break's width now depends on a column's and a column's
 * on the zoom: guess with the density floor, read the column that produces,
 * then solve again with the break that column earns. It converges in one step
 * because the second break width is never smaller than the first.
 */
export function fitPxPerSec(f: Facts, viewport: number, bucketSecs?: number): number {
	const spans = f.activity.spans;
	if (spans.length === 0) return 1;
	const active = spans.reduce((n, s) => n + s.secs, 0);
	if (active <= 0) return 1;
	const d = density(spans.length);
	const solve = (breakPx: number) => {
		const breaks = Math.max(0, spans.length - 1) * breakPx;
		return Math.max(MIN_COL_PX, viewport - breaks - spans.length * MIN_COL_PX) / active;
	};
	const first = solve(BREAK_PX[d]);
	const width = bucketSecs ?? f.activity.bucket_secs;
	const col = resolution(first, width, false).secs * first;
	return solve(breakWidth(f, first, viewport, col));
}

export type ColumnSource = 'facts' | 'events';

export interface Resolution {
	/** Seconds one column covers. */
	secs: number;
	source: ColumnSource;
	/** How many facts buckets a column sums. `1` at the session's own width. */
	buckets: number;
}

/** Rungs finer than a bucket — any width works, the events are re-bucketed. */
const FINE_LADDER: readonly number[] = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900];

/**
 * What one column covers at this zoom, and which document it comes from.
 *
 * Two different thresholds, because they answer two different questions.
 * *Coarsening* is forced: below `MIN_COL_PX` a column is an artefact, so
 * buckets are summed until it is wide enough — a whole number of them, which
 * is what keeps the fit view identical to the report's strip. *Refining* is
 * optional: a bucket is split only when the pieces would still read at
 * `TARGET_COL_PX`, so zooming reveals detail rather than spreading the same
 * detail thinner.
 *
 * `fine` is whether the events are loaded. Until they are there is nothing
 * below `bucket_secs` to show, and the honest answer is the facts' own
 * resolution — not a finer grid drawn from nothing.
 */
export function resolution(pxPerSec: number, bucketSecs: number, fine = true): Resolution {
	const width = Math.max(1, bucketSecs);
	const px = Math.max(pxPerSec, 1e-9);
	const floor = MIN_COL_PX / px;
	if (floor >= width) {
		const k = Math.max(1, Math.ceil(floor / width));
		return { secs: width * k, source: 'facts', buckets: k };
	}
	const target = TARGET_COL_PX / px;
	const rung = fine ? FINE_LADDER.find((s) => s >= target && s < width) : undefined;
	return rung === undefined
		? { secs: width, source: 'facts', buckets: 1 }
		: { secs: rung, source: 'events', buckets: 0 };
}

export interface Column {
	span: number;
	/** Seconds into the span. */
	from: number;
	to: number;
	x: number;
	w: number;
	/** Bar height as a percentage of the track. `0` when nothing is here. */
	pct: number;
	failed: boolean;
	/** RFC 3339 start of the column's window — what a selection is cut from. */
	at: string;
	tip: string;
	value: number;
}

export interface Columns {
	source: ColumnSource;
	secs: number;
	items: Column[];
	peak: number;
	/** What the bar counts. The caption and the legend read this. */
	metric: 'records' | 'events';
	/**
	 * Events whose timestamp fell in no span — counted, never dropped, and
	 * never folded into a column that did not hold them.
	 */
	unplaced: number;
}

/** Empty when the resolution needs the events and they are not loaded yet. */
export function columns(
	f: Facts,
	events: EventsDocument | undefined,
	t: Track,
	res: Resolution
): Columns | undefined {
	if (res.source === 'events' && !events) return undefined;
	return res.source === 'facts' ? factsColumns(f, t, res) : eventColumns(events!, t, res);
}

interface Tally {
	records: number;
	turns: number;
	toolCalls: number;
	failures: number;
	outputTokens: number;
}

function empty(): Tally {
	return { records: 0, turns: 0, toolCalls: 0, failures: 0, outputTokens: 0 };
}

function factsColumns(f: Facts, t: Track, res: Resolution): Columns {
	const width = Math.max(1, f.activity.bucket_secs);
	const items: Column[] = [];
	let peak = 0;
	for (const [si, span] of f.activity.spans.entries()) {
		const ts = t.spans[si];
		for (let j = 0; j < span.buckets.length; j += res.buckets) {
			const chunk = span.buckets.slice(j, j + res.buckets);
			const tally = empty();
			for (const b of chunk) add(tally, b);
			const from = j * width;
			if (from > span.secs && j > 0) break;
			const to = Math.min(from + res.secs, Math.max(span.secs, from));
			peak = Math.max(peak, tally.records);
			items.push(column(ts, from, to, tally, 'records'));
		}
	}
	return finish(items, peak, res, 'records', 0);
}

function add(t: Tally, b: Bucket): void {
	t.records += b.records;
	t.toolCalls += b.tool_calls;
	t.failures += b.tool_failures;
	t.outputTokens += b.output_tokens;
}

function eventColumns(ev: EventsDocument, t: Track, res: Resolution): Columns {
	const tallies = new Map<string, Tally>();
	let unplaced = 0;
	for (const e of ev.events) {
		const at = epoch(e.at);
		if (at === undefined) {
			unplaced += 1;
			continue;
		}
		const ts = spanAt(t, at);
		if (!ts) {
			unplaced += 1;
			continue;
		}
		// The last instant of a span rounds to a column past its end — clamp,
		// rather than tallying an event into a column nothing renders.
		const cols = Math.max(1, Math.ceil(ts.secs / res.secs));
		const j = Math.min(Math.floor((at - ts.startedMs) / 1000 / res.secs), cols - 1);
		const key = `${ts.index}:${j * res.secs}`;
		const tally = tallies.get(key) ?? empty();
		if (isTool(e)) {
			tally.toolCalls += 1;
			if (e.failed) tally.failures += 1;
		} else {
			tally.turns += 1;
			tally.outputTokens += e.tokens?.output ?? 0;
		}
		tallies.set(key, tally);
	}

	const items: Column[] = [];
	let peak = 0;
	for (const ts of t.spans) {
		const cols = Math.max(1, Math.ceil(ts.secs / res.secs));
		for (let j = 0; j < cols; j++) {
			const from = j * res.secs;
			const tally = tallies.get(`${ts.index}:${from}`) ?? empty();
			const value = tally.turns + tally.toolCalls;
			peak = Math.max(peak, value);
			items.push(column(ts, from, Math.min(from + res.secs, ts.secs), tally, 'events'));
		}
	}
	return finish(items, peak, res, 'events', unplaced);
}

function spanAt(t: Track, ms: number): TrackSpan | undefined {
	for (const s of t.spans) {
		if (ms >= s.startedMs && ms <= s.startedMs + s.secs * 1000) return s;
	}
	return undefined;
}

function column(
	ts: TrackSpan,
	from: number,
	to: number,
	tally: Tally,
	metric: 'records' | 'events'
): Column {
	const value = metric === 'records' ? tally.records : tally.turns + tally.toolCalls;
	const at = new Date(ts.startedMs + from * 1000).toISOString();
	const w = ts.scale > 0 ? Math.max(0, to - from) * ts.scale : ts.w;
	return {
		span: ts.index,
		from,
		to,
		x: ts.x + from * ts.scale,
		w,
		pct: 0,
		failed: tally.failures > 0,
		at,
		tip: columnTip(at, tally, metric),
		value
	};
}

function finish(
	items: Column[],
	peak: number,
	res: Resolution,
	metric: 'records' | 'events',
	unplaced: number
): Columns {
	const top = Math.max(1, peak);
	for (const c of items) {
		c.pct = c.value === 0 ? 0 : Math.min(100, MIN_BAR_PCT + Math.ceil((c.value * 94) / top));
	}
	return { source: res.source, secs: res.secs, items, peak, metric, unplaced };
}

function columnTip(at: string, t: Tally, metric: 'records' | 'events'): string {
	const head = metric === 'records' ? `${t.records} record(s)` : `${t.turns} turn(s)`;
	let tip = `${clock(at)} · ${head}`;
	if (t.toolCalls > 0) tip += `, ${t.toolCalls} tool call(s)`;
	if (t.failures > 0) tip += `, ${t.failures} failed`;
	if (t.outputTokens > 0) tip += `, ${count(t.outputTokens)} out`;
	return tip;
}

export interface Band {
	/** Index into `facts.phases` — what a phase selection carries. */
	phase: number;
	kind: PhaseKind;
	x: number;
	w: number;
	tip: string;
	/** The model-written label, when there is one. Marked wherever shown. */
	written?: string;
}

export interface Bands {
	items: Band[];
	/** Phases too narrow to draw at this zoom — counted, never dropped. */
	tooNarrow: number;
}

export function bands(f: Facts, t: Track): Bands {
	const items: Band[] = [];
	let tooNarrow = 0;
	for (const [i, p] of f.phases.entries()) {
		const ts = t.spans[p.span];
		if (!ts) continue;
		const from = offset(p.started, ts);
		const to = offset(p.ended, ts);
		const x = ts.x + from * ts.scale;
		const w = (to - from) * ts.scale;
		if (w <= 0) {
			tooNarrow += 1;
			continue;
		}
		items.push({
			phase: i,
			kind: p.kind,
			x,
			w: Math.min(w, ts.x + ts.w - x),
			tip: phaseTip(f, i),
			written: f.labels?.phases.find((l) => l.phase === i)?.label
		});
	}
	return { items, tooNarrow };
}

function offset(iso: string, ts: TrackSpan): number {
	const at = epoch(iso);
	if (at === undefined) return 0;
	return Math.min(Math.max(0, (at - ts.startedMs) / 1000), ts.secs);
}

export function phaseTip(f: Facts, i: number): string {
	const p = f.phases[i];
	let tip = `${p.kind} · ${clock(p.started)} · ${duration(p.secs)} · ${p.tool_calls} tool call(s)`;
	if (p.tool_failures > 0) tip += `, ${p.tool_failures} failed`;
	if (p.opened_by) tip += ` — ${p.opened_by}`;
	return tip;
}

export interface Mark {
	kind: 'prompt' | 'question';
	x: number;
	tip: string;
	/** Index into `facts.user_involvement`. */
	item: number;
}

/**
 * Each involvement moment where it happened. A moment without a timestamp is
 * simply not placed — inventing one would be the whole project's failure in
 * miniature.
 */
export function marks(f: Facts, t: Track): Mark[] {
	const out: Mark[] = [];
	for (const [i, item] of f.user_involvement.entries()) {
		const at = epoch(item.at);
		if (at === undefined) continue;
		const ts = spanAt(t, at);
		if (!ts) continue;
		out.push({
			kind: item.kind,
			x: ts.x + ((at - ts.startedMs) / 1000) * ts.scale,
			tip:
				item.kind === 'prompt'
					? item.preview || 'user pasted an attachment'
					: `asked: ${item.question}`,
			item: i
		});
	}
	return out;
}

export interface Tick {
	x: number;
	label: string;
}

const TICK_LADDER: readonly number[] = [
	5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400, 21600, 43200, 86400
];

/**
 * Wall-clock ticks inside the spans, at a round interval wide enough to read.
 *
 * Ticks are placed per span rather than across the track: the axis is active
 * seconds, so a tick drawn across a break would be a time that never happened.
 */
export function ticks(t: Track, x0: number, x1: number, minGapPx = 96): Tick[] {
	if (t.pxPerSec <= 0) return [];
	const need = minGapPx / t.pxPerSec;
	const step = TICK_LADDER.find((s) => s >= need);
	if (step === undefined) return [];
	const out: Tick[] = [];
	for (const s of t.spans) {
		if (s.scale <= 0 || s.x + s.w < x0 || s.x > x1) continue;
		// Walk only the part of this span the window shows. Generating every
		// tick of every span and filtering afterwards is two million objects a
		// frame at leaf zoom on a twelve-hour session — the reason this takes
		// the window rather than the caller slicing the result.
		const fromSec = Math.max(0, (x0 - s.x) / s.scale);
		const toMs = s.startedMs + Math.min(s.secs, (x1 - s.x) / s.scale) * 1000;
		let at = Math.ceil((s.startedMs + fromSec * 1000) / 1000 / step) * step * 1000;
		for (; at <= toMs && out.length < 512; at += step * 1000) {
			out.push({ x: s.x + ((at - s.startedMs) / 1000) * s.scale, label: tickLabel(at, step) });
		}
	}
	return out;
}

/**
 * `10:00`, and `10:00:05` once the step is finer than a minute.
 *
 * Found by looking: at one second per column the axis read `19:19` eight times
 * across, which is not a clock, it is noise where a clock should be.
 */
function tickLabel(ms: number, step: number): string {
	const iso = new Date(ms).toISOString();
	return step < 60 ? iso.slice(11, 19) : clock(iso);
}

/** Only what the viewport can show. A track can hold tens of thousands of columns. */
export function visible<T extends { x: number; w?: number }>(
	items: T[],
	x0: number,
	x1: number
): T[] {
	return items.filter((i) => i.x + (i.w ?? 0) >= x0 && i.x <= x1);
}

export interface At {
	span: number;
	/** Seconds into that span. */
	secs: number;
	/** True when `x` landed on a collapsed break rather than on work. */
	inBreak: boolean;
}

/**
 * What is at a track coordinate — the inverse of the layout, and the whole of
 * hit-testing.
 *
 * One handler on the container answers "what did I click?" this way, rather
 * than a listener per rect: a track can hold tens of thousands of columns, and
 * thousands of focusable elements would be as bad for a keyboard as for the
 * frame budget. A break is reported as one, not snapped to a neighbour —
 * clicking the collapsed hours should not select the work either side of them.
 */
export function locate(t: Track, x: number): At | undefined {
	if (t.spans.length === 0) return undefined;
	for (const s of t.spans) {
		if (x < s.x) {
			return { span: s.index, secs: 0, inBreak: s.index > 0 };
		}
		if (x <= s.x + s.w) {
			return { span: s.index, secs: s.scale > 0 ? (x - s.x) / s.scale : 0, inBreak: false };
		}
	}
	const last = t.spans[t.spans.length - 1];
	return { span: last.index, secs: last.secs, inBreak: false };
}

/** Where a moment sits on the track — what keeps a zoom anchored under the cursor. */
export function place(t: Track, at: At): number {
	const s = t.spans[Math.min(Math.max(at.span, 0), t.spans.length - 1)];
	return s ? s.x + Math.min(Math.max(at.secs, 0), s.secs) * s.scale : 0;
}

/** The column a moment falls in, at this resolution — what a click selects. */
export function columnAt(at: At, res: Resolution): { span: number; from: number; to: number } {
	const from = Math.floor(at.secs / res.secs) * res.secs;
	return { span: at.span, from, to: from + res.secs };
}
