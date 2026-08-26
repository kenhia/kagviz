/**
 * What is behind a piece of the timeline — pure, so it can be tested without
 * a DOM, and so the conformance test can hold it to the contract's invariants.
 *
 * A click resolves to a **selection**, and a selection resolves to rows:
 *
 * - a **window** — a column, or a drag across several — is a time range inside
 *   one span, and its events are the ones whose `at` falls in it;
 * - a **phase** is `phase == i` on the events, which is what the contract says
 *   to filter on rather than re-deriving the phase's bounds from its stamps;
 * - a **spawn** is a whole other tier: `spawns[k]` has its own event list, and
 *   none of it carries a `phase` because phases cut the *parent's* timeline.
 *   It is not on the timeline at all, and it is opened from the delegated
 *   tier's row rather than by clicking.
 *
 * Prompts and questions are **not** in the events document and never will be:
 * the facts carry them, with timestamps to merge on. So a segment's rows are
 * the two documents interleaved, which is the only place in the app that reads
 * both for one answer.
 *
 * ## The counts, and why both sides are rendered
 *
 * `#1639`: *the panel's counts must equal the facts' counts it was opened
 * from.* So a segment carries both tiers, and the page renders the facts'
 * figure beside the events'. What matters is which differences are *allowed*,
 * because the two are not the same for every quantity:
 *
 * - **Tool calls and output tokens are an equality.** Both documents count
 *   them on the same record — the call, and the turn. A difference here is a
 *   defect in this app, and `disagreements` says so in those words.
 * - **Failures are not.** The facts count a failure on the **result** record
 *   (`summary.rs`, the `tool_result` branch); the event carries `failed` on
 *   the **call**, stamped with the call's own `at`. A call whose result comes
 *   back after a bucket or phase boundary is therefore counted on one side of
 *   it and drawn on the other. On top of that a failure whose call is not in
 *   the file has no event at all — the facts must not report an unknown as a
 *   zero, and the events have nothing to hang it on.
 *
 * So a failure difference is a `note`: the page says which record each side
 * counted, rather than implying one of them is wrong. Neither is rounded away
 * — that would be the one thing this project does not do.
 */

import { sum } from './contract/decode.js';
import type { Facts, Involvement } from './contract/facts.js';
import type { EventsDocument, SessionEvent, ToolEvent, TurnEvent } from './contract/events.js';
import { isTool } from './contract/events.js';
import { epoch } from './format.js';

export type Selection =
	| { kind: 'window'; span: number; from: number; to: number }
	| { kind: 'phase'; phase: number }
	/** Not on the timeline: phases cut the *parent's* timeline, so a spawn's
	 *  events carry none, and this is opened from the delegated tier's row. */
	| { kind: 'spawn'; spawn: number };

export interface Range {
	fromMs: number;
	toMs: number;
}

/** The wall-clock range a selection covers, for merging the facts' moments in. */
export function range(f: Facts, sel: Selection): Range | undefined {
	if (sel.kind === 'spawn') {
		const sp = f.delegation.spawns[sel.spawn];
		const fromMs = epoch(sp?.started);
		const toMs = epoch(sp?.ended);
		return fromMs === undefined || toMs === undefined ? undefined : { fromMs, toMs };
	}
	if (sel.kind === 'phase') {
		const p = f.phases[sel.phase];
		if (!p) return undefined;
		const fromMs = epoch(p.started);
		const toMs = epoch(p.ended);
		return fromMs === undefined || toMs === undefined ? undefined : { fromMs, toMs };
	}
	const span = f.activity.spans[sel.span];
	if (!span) return undefined;
	const start = epoch(span.started);
	if (start === undefined) return undefined;
	return { fromMs: start + sel.from * 1000, toMs: start + sel.to * 1000 };
}

export type Row =
	| { kind: 'turn'; at?: string; turn: TurnEvent; tools: ToolEvent[] }
	| { kind: 'tool'; at?: string; tool: ToolEvent }
	| { kind: 'said'; at?: string; item: Involvement };

export interface Segment {
	selection: Selection;
	range?: Range;
	rows: Row[];
	counts: Counts;
	/** The same quantities off the facts, when the facts resolve this segment. */
	facts?: FactsCounts;
	/** The two documents counting the same thing and disagreeing — a defect. */
	disagreements: string[];
	/** The two counting *different* things, said in words rather than hidden. */
	notes: string[];
}

export interface Counts {
	turns: number;
	toolCalls: number;
	failures: number;
	opaque: number;
	outputTokens: number;
	files: number;
	linesAdded: number;
	linesDeleted: number;
	prompts: number;
	questions: number;
}

export interface FactsCounts {
	/** Every timestamped record, `system` and snapshot ones included. */
	records: number;
	toolCalls: number;
	failures: number;
	outputTokens: number;
}

export function segment(f: Facts, ev: EventsDocument, sel: Selection): Segment {
	const r = range(f, sel);
	// A spawn's events are its own list, and nothing the *user* said belongs to
	// it: `user_involvement` is the parent's, and a subagent has no user.
	const events =
		sel.kind === 'spawn' ? (ev.spawns[sel.spawn]?.events ?? []) : pick(ev.events, sel, r);
	const rows = merge(events, sel.kind === 'spawn' ? [] : said(f, r));
	const c = counts(events, rows);
	const fc = factsCounts(f, sel);
	return { selection: sel, range: r, rows, counts: c, facts: fc, ...reconcile(c, fc) };
}

/**
 * A phase filters on `phase`, a window on `at` — the contract's own two
 * answers. An event with no timestamp has no phase either and lands in
 * neither, which is correct: it is not that it happened at zero.
 */
function pick(events: SessionEvent[], sel: Selection, r: Range | undefined): SessionEvent[] {
	if (sel.kind === 'phase') return events.filter((e) => e.phase === sel.phase);
	if (!r) return [];
	return events.filter((e) => {
		const at = epoch(e.at);
		return at !== undefined && at >= r.fromMs && at < r.toMs;
	});
}

function said(f: Facts, r: Range | undefined): Involvement[] {
	if (!r) return [];
	return f.user_involvement.filter((i) => {
		const at = epoch(i.at);
		return at !== undefined && at >= r.fromMs && at < r.toMs;
	});
}

/**
 * Turns keep their tool calls; a call whose turn is outside the window stands
 * on its own rather than being hidden or attached to a turn that did not make
 * it. Moments the user had are placed by time among them.
 */
function merge(events: SessionEvent[], moments: Involvement[]): Row[] {
	const rows: Row[] = [];
	for (const e of events) {
		if (isTool(e)) {
			const last = rows[rows.length - 1];
			if (last?.kind === 'turn' && last.tools.length < last.turn.tools) last.tools.push(e);
			else rows.push({ kind: 'tool', at: e.at, tool: e });
		} else {
			rows.push({ kind: 'turn', at: e.at, turn: e, tools: [] });
		}
	}
	for (const item of moments) rows.push({ kind: 'said', at: item.at, item });
	return rows.sort((a, b) => order(a) - order(b) || rank(a) - rank(b));
}

function order(r: Row): number {
	return epoch(r.at) ?? 0;
}

/** A prompt at the same instant as a turn comes first: it is what caused it. */
function rank(r: Row): number {
	return r.kind === 'said' ? 0 : 1;
}

function counts(events: SessionEvent[], rows: Row[]): Counts {
	const files = new Set<string>();
	const c: Counts = {
		turns: 0,
		toolCalls: 0,
		failures: 0,
		opaque: 0,
		outputTokens: 0,
		files: 0,
		linesAdded: 0,
		linesDeleted: 0,
		prompts: 0,
		questions: 0
	};
	for (const e of events) {
		if (isTool(e)) {
			c.toolCalls += 1;
			if (e.failed) c.failures += 1;
			if (e.opaque) c.opaque += 1;
			for (const f of e.files) files.add(f);
			c.linesAdded += e.lines_added ?? 0;
			c.linesDeleted += e.lines_deleted ?? 0;
		} else {
			c.turns += 1;
			c.outputTokens += e.tokens?.output ?? 0;
		}
	}
	c.files = files.size;
	for (const r of rows) {
		if (r.kind !== 'said') continue;
		if (r.item.kind === 'prompt') c.prompts += 1;
		else c.questions += 1;
	}
	return c;
}

/**
 * The facts' own figure for this segment, or `undefined` when the facts do not
 * resolve it — a window finer than `bucket_secs`, or one that does not land on
 * bucket boundaries. There is no honest number to show there, so none is shown.
 */
export function factsCounts(f: Facts, sel: Selection): FactsCounts | undefined {
	if (sel.kind === 'spawn') {
		const sp = f.delegation.spawns[sel.spawn];
		return sp
			? {
					records: sp.records,
					toolCalls: sum(sp.tool_calls),
					failures: sum(sp.tool_failures),
					outputTokens: sp.tokens.output
				}
			: undefined;
	}
	if (sel.kind === 'phase') {
		const p = f.phases[sel.phase];
		return p
			? {
					records: p.records,
					toolCalls: p.tool_calls,
					failures: p.tool_failures,
					outputTokens: p.output_tokens
				}
			: undefined;
	}
	const span = f.activity.spans[sel.span];
	const width = Math.max(1, f.activity.bucket_secs);
	if (!span || sel.from % width !== 0 || (sel.to - sel.from) % width !== 0) return undefined;
	const first = sel.from / width;
	const last = Math.min(span.buckets.length, sel.to / width);
	if (first >= span.buckets.length) return undefined;
	const out: FactsCounts = { records: 0, toolCalls: 0, failures: 0, outputTokens: 0 };
	for (const b of span.buckets.slice(first, last)) {
		out.records += b.records;
		out.toolCalls += b.tool_calls;
		out.failures += b.tool_failures;
		out.outputTokens += b.output_tokens;
	}
	return out;
}

/**
 * The two tiers, side by side — separating a defect from a difference.
 *
 * Nothing here is narrated away: a count this app cannot explain is returned
 * as a `disagreement` and rendered as a warning, because hiding it would make
 * the panel a worse witness than the documents under it.
 */
function reconcile(
	c: Counts,
	fc: FactsCounts | undefined
): { disagreements: string[]; notes: string[] } {
	const disagreements: string[] = [];
	const notes: string[] = [];
	if (!fc) return { disagreements, notes };
	if (fc.toolCalls !== c.toolCalls) {
		disagreements.push(
			`the facts count ${fc.toolCalls} tool call(s) here and the events place ${c.toolCalls}.`
		);
	}
	if (fc.outputTokens !== c.outputTokens) {
		disagreements.push(
			`the facts count ${fc.outputTokens} output token(s) here and the events place ${c.outputTokens}.`
		);
	}
	if (fc.failures !== c.failures) {
		notes.push(
			`${fc.failures} failure(s) were recorded here and ${c.failures} call(s) shown here failed. ` +
				`The two count different records: a failure is counted where its result came back, and ` +
				`a call is drawn where it was made — a call whose result crossed this boundary lands on ` +
				`one side and is drawn on the other. A failure whose call is not in the transcript has ` +
				`no call to hang on at all.`
		);
	}
	return { disagreements, notes };
}

/** `?phase=3`, `?span=0&from=120&to=150` — the selection, in the hash. */
export function toQuery(sel: Selection | undefined): URLSearchParams {
	const q = new URLSearchParams();
	if (!sel) return q;
	if (sel.kind === 'phase') q.set('phase', String(sel.phase));
	else if (sel.kind === 'spawn') q.set('spawn', String(sel.spawn));
	else {
		q.set('span', String(sel.span));
		q.set('from', String(sel.from));
		q.set('to', String(sel.to));
	}
	return q;
}

/** `undefined` for anything this app does not recognise — never a guess. */
export function fromQuery(q: URLSearchParams): Selection | undefined {
	const phase = int(q.get('phase'));
	if (phase !== undefined) return { kind: 'phase', phase };
	const spawn = int(q.get('spawn'));
	if (spawn !== undefined) return { kind: 'spawn', spawn };
	const span = int(q.get('span'));
	const from = int(q.get('from'));
	const to = int(q.get('to'));
	if (span === undefined || from === undefined || to === undefined || to <= from) return undefined;
	return { kind: 'window', span, from, to };
}

function int(v: string | null): number | undefined {
	if (v === null || v.trim() === '') return undefined;
	const n = Number(v);
	return Number.isInteger(n) && n >= 0 ? n : undefined;
}
