/**
 * The events document — `kagviz show <id> --events`, and
 * `derived/events/<host>/<id>.json`.
 *
 * The detail tier under the facts: every assistant turn and every tool call,
 * in time order, each stamped with the phase that holds it. A third contract
 * under the same rules. Typed in sprint 011 before anything read it; sprint
 * 012 is what reads it — `timeline.ts` re-buckets these past the strip's
 * resolution, and `segment.ts` is what a click into one resolves to.
 *
 * The invariants a consumer can lean on (`conformance.spec.ts` asserts every
 * one of them over the repo's goldens):
 *
 * - `tool` events == the facts' `tool_calls` summed; `turn` events ==
 *   `assistant_turns`.
 * - `tool` events with `failed` == `tool_failures` summed **less `<unknown>`**
 *   — a failure whose call is not in the file has no call to hang on.
 * - `tool` events with `opaque` == `changes.opaque_edits`; the line counts sum
 *   to `changes.lines_added`/`lines_deleted`; distinct `files` are
 *   `changes.files_touched`.
 * - For every phase `i`, the events with `phase: i` add up to that phase's
 *   `tool_calls` and `output_tokens` exactly. **Not `tool_failures`**: the
 *   facts count a failure on the record carrying the result and an event
 *   carries `failed` on the call, so a call whose result crossed the boundary
 *   is counted in one phase and drawn in the next — either way round. Only
 *   the sum across the phases is fixed, and it falls short by `<unknown>`.
 *   Same per spawn.
 */

import {
	arrOr,
	boolOr,
	ContractError,
	numOr,
	object,
	optNum,
	optStr,
	str,
	strItem
} from './decode.js';
import { decodeTokens, type ToolClass, type Tokens } from './facts.js';

export interface TurnEvent {
	kind: 'turn';
	/** **Absent** when the record had none — and then so is `phase`. */
	at?: string;
	phase?: number;
	model?: string;
	tokens?: Tokens;
	/** How many `tool` events follow this turn. */
	tools: number;
}

export interface ToolEvent {
	kind: 'tool';
	at?: string;
	/** Absent on every event of a spawn, and on an untimestamped record. */
	phase?: number;
	tool: string;
	/** How the phase mix classified it — the same table `mix` uses. */
	class: ToolClass;
	id?: string;
	/** The call's input re-serialized compactly — a canonical size. */
	input_bytes?: number;
	result_at?: string;
	/** Present only when the result came back `is_error`. */
	failed: boolean;
	result_bytes?: number;
	/** Named when the result named them; absent when empty. */
	files: string[];
	/** Present when a diff was read, **absent** when not — never a zero. */
	lines_added?: number;
	lines_deleted?: number;
	/** True when this call is one of `changes.opaque_edits`. */
	opaque: boolean;
}

export type SessionEvent = TurnEvent | ToolEvent;

export interface SpawnEvents {
	agent_id?: string;
	events: SessionEvent[];
}

export interface EventsDocument {
	session_id?: string;
	events: SessionEvent[];
	/** One per `delegation.spawns[]`, same order. */
	spawns: SpawnEvents[];
}

const TOOL_CLASSES: readonly string[] = ['read', 'edit', 'run', 'org', 'ask', 'delegate', 'other'];

export function decodeEvents(raw: unknown, path = 'events'): EventsDocument {
	const o = object(raw, path);
	return {
		session_id: optStr(o, 'session_id', path),
		events: arrOr(o, 'events', path, decodeEvent),
		spawns: arrOr(o, 'spawns', path, (v, p) => {
			const s = object(v, p);
			return {
				agent_id: optStr(s, 'agent_id', p),
				events: arrOr(s, 'events', p, decodeEvent)
			};
		})
	};
}

export function decodeEvent(raw: unknown, path: string): SessionEvent {
	const o = object(raw, path);
	const kind = str(o, 'kind', path);
	if (kind === 'turn') {
		return {
			kind: 'turn',
			at: optStr(o, 'at', path),
			phase: optNum(o, 'phase', path),
			model: optStr(o, 'model', path),
			tokens: o['tokens'] == null ? undefined : decodeTokens(o['tokens'], `${path}.tokens`),
			tools: numOr(o, 'tools', path, 0)
		};
	}
	if (kind === 'tool') {
		const cls = str(o, 'class', path);
		if (!TOOL_CLASSES.includes(cls)) {
			throw new ContractError(`${path}.class`, `unknown tool class ${JSON.stringify(cls)}`);
		}
		return {
			kind: 'tool',
			at: optStr(o, 'at', path),
			phase: optNum(o, 'phase', path),
			tool: str(o, 'tool', path),
			class: cls as ToolClass,
			id: optStr(o, 'id', path),
			input_bytes: optNum(o, 'input_bytes', path),
			result_at: optStr(o, 'result_at', path),
			failed: boolOr(o, 'failed', path, false),
			result_bytes: optNum(o, 'result_bytes', path),
			files: arrOr(o, 'files', path, strItem),
			lines_added: optNum(o, 'lines_added', path),
			lines_deleted: optNum(o, 'lines_deleted', path),
			opaque: boolOr(o, 'opaque', path, false)
		};
	}
	throw new ContractError(`${path}.kind`, `unknown event kind ${JSON.stringify(kind)}`);
}

/** Narrowing helpers, so a filter reads as what it filters for. */
export function isTool(e: SessionEvent): e is ToolEvent {
	return e.kind === 'tool';
}

export function isTurn(e: SessionEvent): e is TurnEvent {
	return e.kind === 'turn';
}
