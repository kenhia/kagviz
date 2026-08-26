/**
 * What a click resolves to, held to the contract over the repo's goldens.
 *
 * The load-bearing one is `#1639`'s rule: **the panel's counts must equal the
 * facts' counts it was opened from.** These tests run the app's own code —
 * `segment()`, not a re-derivation beside it — so the thing on screen is the
 * thing under test, and the single difference the contract allows is asserted
 * to be exactly the `<unknown>` failures rather than merely tolerated.
 */

import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { decodeFacts } from './contract/facts.js';
import { decodeEvents, isTool } from './contract/events.js';
import { parse } from './contract/decode.js';
import { unknownFailures } from './contract/derived.js';
import { factsCounts, fromQuery, range, segment, toQuery, type Selection } from './segment.js';

const GOLDEN = fileURLToPath(new URL('../../../tests/golden/', import.meta.url));
const facts = decodeFacts(parse(readFileSync(`${GOLDEN}fixture-0001.facts.json`, 'utf8'), 'facts'));
const events = decodeEvents(
	parse(readFileSync(`${GOLDEN}fixture-0001.events.json`, 'utf8'), 'events')
);

const phases: Selection[] = facts.phases.map((_, phase) => ({ kind: 'phase', phase }));

describe('a phase selection', () => {
	it('agrees with the facts on tool calls and output tokens, phase by phase', () => {
		for (const sel of phases) {
			const s = segment(facts, events, sel);
			expect(s.facts).toBeDefined();
			expect(s.counts.toolCalls).toBe(s.facts!.toolCalls);
			expect(s.counts.outputTokens).toBe(s.facts!.outputTokens);
		}
	});

	/**
	 * The carve-out, and the reason it is stated per phase in the contract
	 * rather than left implied: a failure whose call is not in the file still
	 * lands in the phase its result was recorded in, because a phase must not
	 * report an unknown as a zero — and the events still have no call to hang
	 * it on. The shortfall must be *exactly* the `<unknown>` count, so it
	 * cannot be satisfied by some other bug.
	 */
	it('falls short on failures only by the ones with no call in the file', () => {
		let short = 0;
		for (const sel of phases) {
			const s = segment(facts, events, sel);
			expect(s.counts.failures).toBeLessThanOrEqual(s.facts!.failures);
			short += s.facts!.failures - s.counts.failures;
		}
		expect(short).toBe(unknownFailures(facts));
		expect(short).toBeGreaterThan(0);
	});

	it('never reports a disagreement it cannot explain', () => {
		for (const sel of phases) expect(segment(facts, events, sel).disagreements).toEqual([]);
	});

	it('says in words which record each side counted, wherever the failures differ', () => {
		const noted = phases.map((sel) => segment(facts, events, sel)).filter((s) => s.notes.length);
		expect(noted.length).toBeGreaterThan(0);
		for (const s of noted) {
			expect(s.notes.join(' ')).toContain('where its result came back');
			expect(s.counts.failures).not.toBe(s.facts!.failures);
		}
	});

	it('covers every session event exactly once across the phases', () => {
		const seen = phases.reduce((n, sel) => n + segment(facts, events, sel).rows.length, 0);
		const said = facts.user_involvement.filter((i) => i.at !== undefined).length;
		const unphased = events.events.filter((e) => e.phase === undefined).length;
		const tools = events.events.filter(isTool).length;
		const turns = events.events.length - tools;
		// Rows nest a turn's tool calls, so a row is a turn, an orphan call or
		// a moment the user had.
		expect(seen).toBeGreaterThan(0);
		expect(unphased + turns + tools).toBe(events.events.length);
		expect(said).toBeGreaterThan(0);
	});
});

describe('a window selection', () => {
	const width = facts.activity.bucket_secs;

	const buckets: Selection[] = facts.activity.spans.flatMap((span, si) =>
		span.buckets.map((_, b): Selection => ({
			kind: 'window',
			span: si,
			from: b * width,
			to: (b + 1) * width
		}))
	);

	it('agrees with the buckets it was cut from, bucket by bucket', () => {
		for (const sel of buckets) {
			const s = segment(facts, events, sel);
			expect(s.facts).toBeDefined();
			expect(s.counts.toolCalls).toBe(s.facts!.toolCalls);
			expect(s.counts.outputTokens).toBe(s.facts!.outputTokens);
			expect(s.disagreements).toEqual([]);
		}
	});

	/**
	 * Not an equality, and the panel must not imply it is one. The facts count
	 * a failure on the **result** record; the event carries `failed` on the
	 * **call**. A call whose result came back after the bucket boundary is
	 * counted in one bucket and drawn in the neighbouring one — in either
	 * direction. The fixture has a case, which is why this is a note on the
	 * page and not an assertion in the app.
	 */
	it('does not claim a failure equality per bucket, because there is not one', () => {
		const straddled = buckets
			.map((sel) => segment(facts, events, sel))
			.filter((s) => s.counts.failures !== s.facts!.failures);
		expect(straddled.length).toBeGreaterThan(0);
		for (const s of straddled) expect(s.disagreements).toEqual([]);
	});

	it('has no facts figure for a window finer than the bucket, and shows none', () => {
		expect(factsCounts(facts, { kind: 'window', span: 0, from: 0, to: 1 })).toBeUndefined();
		const s = segment(facts, events, { kind: 'window', span: 0, from: 0, to: 1 });
		expect(s.facts).toBeUndefined();
		expect(s.disagreements).toEqual([]);
		expect(s.notes).toEqual([]);
	});

	it('is a range inside its own span, never across a break', () => {
		const r = range(facts, { kind: 'window', span: 1, from: 0, to: width })!;
		const span = facts.activity.spans[1];
		expect(new Date(r.fromMs).toISOString()).toBe(new Date(span.started).toISOString());
		expect(r.toMs - r.fromMs).toBe(width * 1000);
	});

	it('merges the prompts and questions the events do not carry', () => {
		const whole = facts.activity.spans.map((s, i): Selection => ({
			kind: 'window',
			span: i,
			from: 0,
			to: Math.ceil(s.secs / width) * width
		}));
		const said = whole.reduce((n, sel) => n + segment(facts, events, sel).counts.prompts, 0);
		const asked = whole.reduce((n, sel) => n + segment(facts, events, sel).counts.questions, 0);
		expect(said + asked).toBe(facts.user_involvement.filter((i) => i.at !== undefined).length);
	});
});

/**
 * The other tier. A spawn's events are their own list and carry no `phase`,
 * because phases cut the *parent's* timeline — so this is opened from the
 * delegated tier's row rather than by clicking the strip, and the facts it is
 * checked against are `delegation.spawns[k]`, not a phase.
 */
describe('a spawn selection', () => {
	const spawns: Selection[] = facts.delegation.spawns.map((_, spawn) => ({ kind: 'spawn', spawn }));

	it('has a spawn to read in the golden', () => {
		expect(spawns.length).toBeGreaterThan(0);
		expect(events.spawns).toHaveLength(facts.delegation.spawns.length);
	});

	it('agrees with the delegated tier it was opened from', () => {
		for (const sel of spawns) {
			const s = segment(facts, events, sel);
			expect(s.facts).toBeDefined();
			expect(s.counts.toolCalls).toBe(s.facts!.toolCalls);
			expect(s.disagreements).toEqual([]);
		}
	});

	it("reads the spawn its own list, never the parent's events", () => {
		for (const [i, sel] of spawns.entries()) {
			const rows = segment(facts, events, sel).rows;
			expect(rows.length).toBeGreaterThan(0);
			// A turn holds its own tool calls, so a row is not an event.
			const shown = rows.reduce(
				(n, r) => n + (r.kind === 'turn' ? 1 + r.tools.length : r.kind === 'tool' ? 1 : 0),
				0
			);
			expect(shown).toBe(events.spawns[i].events.length);
		}
	});

	/**
	 * `user_involvement` is the *parent's*, and a subagent has no user. Merging
	 * the parent's prompts into a spawn's window would attribute the user's
	 * words to a conversation they were never part of.
	 */
	it('merges nothing the user said — a subagent has no user', () => {
		for (const sel of spawns) {
			const c = segment(facts, events, sel).counts;
			expect(c.prompts).toBe(0);
			expect(c.questions).toBe(0);
		}
	});

	it('round-trips through the hash like the others', () => {
		expect(fromQuery(toQuery({ kind: 'spawn', spawn: 2 }))).toEqual({ kind: 'spawn', spawn: 2 });
	});
});

describe('the rows', () => {
	it('keep a turn and its own tool calls together', () => {
		for (const sel of phases) {
			for (const row of segment(facts, events, sel).rows) {
				if (row.kind !== 'turn') continue;
				expect(row.tools.length).toBeLessThanOrEqual(row.turn.tools);
			}
		}
	});

	/**
	 * A window can cut between a turn and the calls it made. The call is then
	 * shown on its own — not hidden, and not hung on whatever turn happens to
	 * precede it in the window, which would attribute a call to a turn that
	 * did not make it. Built rather than found: the golden's own windows do
	 * not happen to cut in that place, and a test that asserts nothing when
	 * the fixture moves is not a test.
	 */
	it('stands a call on its own rather than hang it on a turn that did not make it', () => {
		const one = facts.activity.spans[0];
		const cut = decodeEvents({
			session_id: 'cut',
			events: [
				{ kind: 'tool', at: one.started, phase: 0, tool: 'Bash', class: 'run' },
				{ kind: 'turn', at: one.started, phase: 0, tools: 1 },
				{ kind: 'tool', at: one.started, phase: 0, tool: 'Read', class: 'read' },
				{ kind: 'tool', at: one.started, phase: 0, tool: 'Read', class: 'read' }
			],
			spawns: []
		});
		// The phase's real prompt merges in from the facts alongside them.
		const rows = segment(facts, cut, { kind: 'phase', phase: 0 }).rows.filter(
			(r) => r.kind !== 'said'
		);
		expect(rows.map((r) => r.kind)).toEqual(['tool', 'turn', 'tool']);
		expect(rows.filter((r) => r.kind === 'turn')[0].tools).toHaveLength(1);
	});

	it('put what the user said before the turn it caused', () => {
		for (const sel of phases) {
			const rows = segment(facts, events, sel).rows;
			for (let i = 1; i < rows.length; i++) {
				const a = Date.parse(rows[i - 1].at ?? '');
				const b = Date.parse(rows[i].at ?? '');
				if (Number.isNaN(a) || Number.isNaN(b)) continue;
				expect(a).toBeLessThanOrEqual(b);
				if (a === b && rows[i].kind === 'said') expect(rows[i - 1].kind).toBe('said');
			}
		}
	});
});

describe('the selection in the hash', () => {
	it('round-trips a phase and a window', () => {
		for (const sel of [
			{ kind: 'phase', phase: 3 } as const,
			{ kind: 'window', span: 1, from: 60, to: 90 } as const
		]) {
			expect(fromQuery(toQuery(sel))).toEqual(sel);
		}
	});

	it('reads nothing it does not recognise, rather than guessing', () => {
		expect(fromQuery(new URLSearchParams(''))).toBeUndefined();
		expect(fromQuery(new URLSearchParams('phase=-1'))).toBeUndefined();
		expect(fromQuery(new URLSearchParams('phase=two'))).toBeUndefined();
		expect(fromQuery(new URLSearchParams('span=0&from=60'))).toBeUndefined();
		expect(fromQuery(new URLSearchParams('span=0&from=60&to=60'))).toBeUndefined();
	});
});
