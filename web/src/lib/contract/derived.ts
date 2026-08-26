/**
 * The quantities the contract says a consumer **may** recompute, in one place.
 *
 * `total_tool_calls`, `tool_failure_rate` and the `combined*` family are
 * methods on the Rust side rather than serialized fields, because the facts
 * carry each tier once and a sum anyone can recompute is not a separate fact.
 * What is *not* optional is showing them — so the app needs the same
 * arithmetic, and it must make the same choices the report does or its numbers
 * will disagree with the report's over the same session.
 *
 * Two choices in particular, both from `docs/facts-contract.md`:
 *
 * - The failure rate's denominator is `tool_calls` — a failed call is a call,
 *   counted once — and failures blamed on `<unknown>` are **out of the
 *   numerator**, since their calls are not in the denominator either.
 * - There is no `combinedActiveSecs`, deliberately. A subagent runs while the
 *   session waits on it, so those seconds overlap rather than add. Tokens add
 *   across concurrent agents; seconds do not.
 *
 * `conformance.spec.ts` checks each of these against the numbers in the
 * golden `fixture-0001.show.txt`, which is the report's own arithmetic.
 */

import { sum } from './decode.js';
import type { Facts, PhaseKind } from './facts.js';

export function totalToolCalls(f: Facts): number {
	return sum(f.tool_calls);
}

export function totalToolFailures(f: Facts): number {
	return sum(f.tool_failures);
}

/** Failures kagviz counted but could not place — their call is not in the file. */
export function unknownFailures(f: Facts): number {
	return f.tool_failures['<unknown>'] ?? 0;
}

/**
 * The share of this session's tool calls that failed, or `undefined` when
 * there is nothing worth dividing: no calls, or no failure that joined to one.
 * The zero case reads as "none failed"; a `0.00%` beside that is noise.
 */
export function toolFailureRate(f: Facts): number | undefined {
	const calls = totalToolCalls(f);
	const joined = Math.max(0, totalToolFailures(f) - unknownFailures(f));
	return calls > 0 && joined > 0 ? joined / calls : undefined;
}

export function combinedToolCalls(f: Facts): number {
	return totalToolCalls(f) + sum(f.delegation.totals.tool_calls);
}

export function combinedToolFailures(f: Facts): number {
	return totalToolFailures(f) + sum(f.delegation.totals.tool_failures);
}

export function combinedOutputTokens(f: Facts): number {
	return f.tokens.output + f.delegation.totals.tokens.output;
}

/** How many agents this session handed work to, joined or not. */
export function delegatedAgents(f: Facts): number {
	return f.delegation.spawns.length + f.delegation.unjoined_spawns;
}

export function hasDelegation(f: Facts): boolean {
	return f.delegation.spawns.length > 0 || f.delegation.unjoined_spawns > 0;
}

export interface PhaseRollupRow {
	kind: PhaseKind;
	phases: number;
	secs: number;
}

/**
 * Phase kinds by time spent, largest first — the report's Phases rollup.
 * Ties break on phase count, then on the kind's own order, so two renderings
 * of one session cannot disagree about the row order.
 */
export function phaseRollup(f: Facts): PhaseRollupRow[] {
	const by = new Map<PhaseKind, PhaseRollupRow>();
	for (const p of f.phases) {
		const row = by.get(p.kind) ?? { kind: p.kind, phases: 0, secs: 0 };
		row.phases += 1;
		row.secs += p.secs;
		by.set(p.kind, row);
	}
	return [...by.values()].sort(
		(a, b) => b.secs - a.secs || b.phases - a.phases || KIND_ORDER[a.kind] - KIND_ORDER[b.kind]
	);
}

export function dominantPhase(f: Facts): PhaseKind | undefined {
	return phaseRollup(f)[0]?.kind;
}

/** The declaration order of `PhaseKind` on the Rust side, which is its `Ord`. */
const KIND_ORDER: Record<PhaseKind, number> = {
	exploring: 0,
	implementing: 1,
	running: 2,
	filing: 3,
	delegating: 4,
	discussing: 5,
	mixed: 6
};

/** The label a model wrote for phase `i`, if it wrote one. Sparse by design. */
export function phaseLabel(f: Facts, i: number): string | undefined {
	return f.labels?.phases.find((p) => p.phase === i)?.label;
}
