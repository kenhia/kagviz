/**
 * The test that makes `contract/` a contract rather than a transcription.
 *
 * It reads the repo's own goldens — `tests/golden/fixture-0001.{facts,events,
 * sessions}.json`, the bytes the Rust binary actually emits, checked in and
 * regenerated with `KAGVIZ_UPDATE_GOLDEN=1` — puts them through the decoders,
 * and asserts the invariants `docs/facts-contract.md` states. It runs inside
 * `just check` and CI, so the app is the contract's **second consumer in the
 * gate**: a facts change that breaks the app fails the build on the Rust side
 * too, the day it lands, rather than the next time someone opens the page.
 *
 * The numbers it checks the derived helpers against come from
 * `fixture-0001.show.txt`, the terminal view — the report's own arithmetic,
 * not a number restated here.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

import { ContractError, parse, sum } from './decode.js';
import { decodeFacts, type Facts } from './facts.js';
import { decodeEvents, isTool, isTurn, type ToolEvent } from './events.js';
import { decodeSessions, decodeSyncStatus } from './sessions.js';
import * as derivedModule from './derived.js';
import {
	combinedOutputTokens,
	combinedToolCalls,
	combinedToolFailures,
	phaseRollup,
	toolFailureRate,
	totalToolCalls,
	totalToolFailures,
	unknownFailures
} from './derived.js';

const GOLDEN = fileURLToPath(new URL('../../../../tests/golden/', import.meta.url));

function golden(name: string): string {
	return readFileSync(`${GOLDEN}${name}`, 'utf8');
}

const factsText = golden('fixture-0001.facts.json');
const eventsText = golden('fixture-0001.events.json');
const sessionsText = golden('fixture-0001.sessions.json');
const showText = golden('fixture-0001.show.txt');

const facts = decodeFacts(parse(factsText, 'facts'));
const events = decodeEvents(parse(eventsText, 'events'));
const index = decodeSessions(parse(sessionsText, 'sessions.json'));

const tools = events.events.filter(isTool);
const turns = events.events.filter(isTurn);

describe('the facts document', () => {
	it('decodes the golden', () => {
		expect(facts.session_id).toBe('fixture-0001');
		expect(facts.phases.length).toBeGreaterThan(0);
		expect(facts.activity.spans.length).toBeGreaterThan(0);
	});

	it('states optional fields as absent, never null', () => {
		// The producer's half of the rule, held on the bytes. The decoders
		// fold `null` into `undefined` because the contract tells a *consumer*
		// to read the two alike — this is what keeps kagviz from needing that.
		expect(factsText).not.toContain('null');
		expect(eventsText).not.toContain('null');
		expect(sessionsText).not.toContain('null');
	});

	it('leaves a resumed phase without an opened_by rather than blank', () => {
		const resumed = facts.phases.filter((p) => p.opened_by === undefined);
		expect(resumed.length).toBeGreaterThan(0);
		for (const p of resumed) expect('opened_by' in p && p.opened_by === '').toBe(false);
	});

	it('carries an unanswered question without a chosen option', () => {
		const questions = facts.user_involvement.filter((i) => i.kind === 'question');
		expect(questions.length).toBeGreaterThan(0);
		// The fixture's one question was answered; what matters is that the
		// field is optional in the type, so an interrupted one cannot be
		// rendered as a default choice.
		for (const q of questions) {
			if (q.kind === 'question' && q.chosen !== undefined) {
				expect(q.options).toContain(q.chosen);
			}
		}
	});

	it('keeps opaque_edits out of the line deltas', () => {
		// The floor rule: by_tool sums back to the totals exactly, and a tool
		// can be opaque *and* have contributed files.
		let added = 0;
		let deleted = 0;
		let opaque = 0;
		for (const t of Object.values(facts.changes.by_tool)) {
			added += t.lines_added;
			deleted += t.lines_deleted;
			opaque += t.opaque;
		}
		expect(added).toBe(facts.changes.lines_added);
		expect(deleted).toBe(facts.changes.lines_deleted);
		expect(opaque).toBe(facts.changes.opaque_edits);
		expect(facts.changes.opaque_edits).toBeGreaterThan(0);
	});

	it('carries no labels unless the facts were labelled', () => {
		expect(facts.labels).toBeUndefined();
	});

	it('is a session whose phases account for every second of their span', () => {
		for (const [i, span] of facts.activity.spans.entries()) {
			const inSpan = facts.phases.filter((p) => p.span === i);
			expect(inSpan.reduce((n, p) => n + p.secs, 0)).toBe(span.secs);
		}
		// And since 005, the spans are where active_secs is read off.
		expect(facts.activity.spans.reduce((n, s) => n + s.secs, 0)).toBe(facts.active_secs);
	});
});

describe('the events document', () => {
	it('has one tool event per counted tool call, and one turn per assistant turn', () => {
		expect(tools.length).toBe(sum(facts.tool_calls));
		expect(turns.length).toBe(facts.assistant_turns);
	});

	it('places every failure except the ones with no call to hang on', () => {
		const unknown = facts.tool_failures['<unknown>'] ?? 0;
		expect(unknown).toBeGreaterThan(0);
		expect(tools.filter((t) => t.failed).length).toBe(sum(facts.tool_failures) - unknown);
	});

	it('marks exactly the opaque calls the facts count', () => {
		expect(tools.filter((t) => t.opaque).length).toBe(facts.changes.opaque_edits);
	});

	it('sums to the facts line deltas and distinct files', () => {
		const added = tools.reduce((n, t) => n + (t.lines_added ?? 0), 0);
		const deleted = tools.reduce((n, t) => n + (t.lines_deleted ?? 0), 0);
		expect(added).toBe(facts.changes.lines_added);
		expect(deleted).toBe(facts.changes.lines_deleted);
		const files = new Set(tools.flatMap((t) => t.files));
		expect(files.size).toBe(facts.changes.files_touched);
	});

	/**
	 * Calls and tokens are an equality per phase; failures are **not**, and
	 * this test asserted that they were until sprint 012 measured it. The
	 * facts count a failure on the record carrying the *result*, an event
	 * carries `failed` on the *call*: a call whose result came back after the
	 * phase boundary is counted in one phase and drawn in the next, in either
	 * direction. 17 phases of the 413-session corpus place more failures than
	 * their phase counts. The fixture has none, which is the only reason the
	 * old `placed <= phase.tool_failures` held here and in `tests/golden.rs`.
	 * What is true is the signed sum across the phases.
	 */
	it('adds up per phase — exactly for calls and tokens, in sum for failures', () => {
		let unplaced = 0;
		for (const [i, phase] of facts.phases.entries()) {
			const inPhase = tools.filter((t) => t.phase === i);
			expect(inPhase.length).toBe(phase.tool_calls);
			unplaced += phase.tool_failures - inPhase.filter((t) => t.failed).length;
			const output = turns
				.filter((t) => t.phase === i)
				.reduce((n, t) => n + (t.tokens?.output ?? 0), 0);
			expect(output).toBe(phase.output_tokens);
		}
		expect(unplaced).toBe(unknownFailures(facts));
	});

	it('adds up per spawn, in the same order as the facts', () => {
		expect(events.spawns.length).toBe(facts.delegation.spawns.length);
		for (const [i, spawn] of facts.delegation.spawns.entries()) {
			const e = events.spawns[i];
			expect(e.agent_id).toBe(spawn.agent_id);
			expect(e.events.filter(isTool).length).toBe(sum(spawn.tool_calls));
		}
	});

	it('gives a spawn event no phase — phases cut the parent timeline', () => {
		for (const spawn of events.spawns) {
			for (const e of spawn.events) expect(e.phase).toBeUndefined();
		}
	});

	it('leaves a line count absent rather than zero when no diff was read', () => {
		const opaqueShellCall = tools.find((t: ToolEvent) => t.opaque && t.tool === 'Bash');
		expect(opaqueShellCall).toBeDefined();
		expect(opaqueShellCall?.lines_added).toBeUndefined();
		expect(opaqueShellCall?.lines_deleted).toBeUndefined();
	});

	// Sprint 013: a shell call is no longer opaque by virtue of being a shell
	// call. A consumer that still assumes `tool === 'Bash'` implies `opaque`,
	// or that `opaque === calls` for a shell tool, is wrong on this golden.
	it('leaves a shell call that provably wrote nothing off opaque_edits', () => {
		const readOnly = tools.filter((t: ToolEvent) => t.tool === 'Bash' && !t.opaque);
		expect(readOnly.length).toBeGreaterThan(0);
		const bash = facts.changes.by_tool['Bash'];
		expect(bash.opaque).toBeLessThan(bash.calls);
		expect(bash.calls - bash.opaque).toBe(readOnly.length);
	});
});

describe('sessions.json', () => {
	it('decodes the golden and copies the facts it claims to copy', () => {
		expect(index.sessions).toHaveLength(1);
		const row = index.sessions[0];
		expect(row.host).toBe('kai');
		expect(row.session_id).toBe(facts.session_id);
		expect(row.project).toBe(facts.project);
		expect(row.cwd).toBe(facts.cwd);
		expect(row.git_branch).toBe(facts.git_branch);
		expect(row.started).toBe(facts.started);
		expect(row.ended).toBe(facts.ended);
		expect(row.wall_secs).toBe(facts.wall_secs);
		expect(row.active_secs).toBe(facts.active_secs);
		expect(row.user_prompts).toBe(facts.user_prompts);
		expect(row.assistant_turns).toBe(facts.assistant_turns);
		expect(row.skipped_lines).toBe(facts.skipped_lines);
		expect(row.phases).toBe(facts.phases.length);
		expect(row.output_tokens).toBe(facts.tokens.output);
		expect(row.files_touched).toBe(facts.changes.files_touched);
		expect(row.lines_added).toBe(facts.changes.lines_added);
		expect(row.lines_deleted).toBe(facts.changes.lines_deleted);
		expect(row.opaque_edits).toBe(facts.changes.opaque_edits);
		expect(row.models).toEqual(Object.keys(facts.models));
		expect(row.cli_versions).toEqual(facts.cli_versions);
	});

	it('sums the session own tier only — delegated work is a count of agents', () => {
		const row = index.sessions[0];
		expect(row.tool_calls).toBe(totalToolCalls(facts));
		expect(row.tool_failures).toBe(totalToolFailures(facts));
		expect(row.delegated_spawns).toBe(facts.delegation.spawns.length);
		// Which is emphatically not the combined figure.
		expect(row.tool_calls).not.toBe(combinedToolCalls(facts));
	});

	it('has no headline, because the fixture facts carry no labels', () => {
		expect(index.sessions[0].headline).toBeUndefined();
		expect(facts.labels).toBeUndefined();
	});

	it('links the three documents relative to the derived root', () => {
		const row = index.sessions[0];
		expect(row.facts).toBe(`facts/${row.host}/${row.session_id}.json`);
		expect(row.events).toBe(`events/${row.host}/${row.session_id}.json`);
		expect(row.report).toBe(`reports/${row.host}/${row.session_id}.html`);
	});

	it('reads a sync status that names an unreachable host', () => {
		const status = decodeSyncStatus(
			parse(
				JSON.stringify({
					ran_at: '2026-08-25T11:00:02Z',
					hosts: {
						kai: { status: 'ok', transferred: 12, secs: 3 },
						cleo: { status: 'unreachable', transferred: 0, secs: 0, note: 'did not answer ssh' }
					}
				}),
				'sync-status.json'
			)
		);
		expect(status.hosts.cleo.status).toBe('unreachable');
		expect(status.hosts.cleo.note).toBe('did not answer ssh');
	});
});

describe('the derived helpers', () => {
	// Read off fixture-0001.show.txt, the terminal view, so these are the
	// report's numbers rather than numbers restated in a test.
	const show = showText;

	it('computes the totals the terminal view prints', () => {
		expect(show).toContain(`${totalToolCalls(facts)} calls`);
		expect(totalToolCalls(facts)).toBe(16);
		expect(totalToolFailures(facts)).toBe(2);
		expect(unknownFailures(facts)).toBe(1);
	});

	it('leaves <unknown> failures out of the failure rate numerator', () => {
		const rate = toolFailureRate(facts);
		expect(rate).toBeDefined();
		// One joined failure of sixteen calls — the unknown one is in neither
		// the numerator nor the denominator.
		expect(rate).toBeCloseTo(1 / 16, 10);
		expect(rate).not.toBeCloseTo(2 / 16, 10);
	});

	it('states the combined tier the report states', () => {
		expect(combinedToolCalls(facts)).toBe(
			totalToolCalls(facts) + sum(facts.delegation.totals.tool_calls)
		);
		expect(combinedToolFailures(facts)).toBe(
			totalToolFailures(facts) + sum(facts.delegation.totals.tool_failures)
		);
		expect(combinedOutputTokens(facts)).toBe(
			facts.tokens.output + facts.delegation.totals.tokens.output
		);
		expect(show).toContain(`${combinedToolCalls(facts)} tool call(s)`);
	});

	it('offers no combined active time, because seconds do not add', () => {
		// A subagent runs while the session waits on it. Asserted as an
		// absence so the day someone adds one, this fails and they read why.
		const helpers = Object.keys(derivedModule as unknown as Record<string, unknown>);
		expect(helpers.some((k) => /combined.*[Aa]ctive/i.test(k))).toBe(false);
	});

	it('rolls phases up by time spent, largest first', () => {
		const rows = phaseRollup(facts);
		expect(rows.length).toBeGreaterThan(0);
		expect(rows.reduce((n, r) => n + r.secs, 0)).toBe(facts.active_secs);
		expect(rows.reduce((n, r) => n + r.phases, 0)).toBe(facts.phases.length);
		for (let i = 1; i < rows.length; i++)
			expect(rows[i - 1].secs).toBeGreaterThanOrEqual(rows[i].secs);
	});
});

describe('a document this app does not understand', () => {
	it('throws with the path that failed rather than rendering zeros', () => {
		expect(() => decodeFacts({ wall_secs: 'soon' })).toThrow(ContractError);
		try {
			decodeFacts({ wall_secs: 'soon' });
		} catch (e) {
			expect((e as ContractError).path).toBe('facts.wall_secs');
		}
	});

	it('rejects a phase kind it has never heard of', () => {
		const broken = JSON.parse(factsText) as { phases: { kind: string }[] };
		broken.phases[0].kind = 'vibing';
		expect(() => decodeFacts(broken)).toThrow(/unknown phase kind/);
	});

	it('ignores a field it does not know, because adding one is not breaking', () => {
		const extended = { ...(JSON.parse(factsText) as object), a_field_from_sprint_099: 42 };
		const decoded = decodeFacts(extended) as Facts & { a_field_from_sprint_099?: number };
		expect(decoded.a_field_from_sprint_099).toBeUndefined();
		expect(decoded.wall_secs).toBe(facts.wall_secs);
	});

	it('reads a null the way it reads an absence', () => {
		const nulled = { ...(JSON.parse(factsText) as object), git_branch: null };
		expect(decodeFacts(nulled).git_branch).toBeUndefined();
	});
});
