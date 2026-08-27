/**
 * The facts document — `kagviz show <id> --json`, and the file
 * `derived/facts/<host>/<id>.json` holds.
 *
 * Written from `docs/facts-contract.md`, and the rules it states are part of
 * the type, not a footnote:
 *
 * - **Every optional is `?:`, never `| null`.** An absent field is absent.
 * - **`opaque_edits` is an unknown, not a zero.** `lines_added` is a floor
 *   wherever it is non-zero, and a renderer must say so.
 * - **`labels` is the only model-written field.** Everything outside it was
 *   counted. It is absent unless the facts were labelled.
 * - **`active_secs` does not combine across tiers** — a subagent runs while
 *   the session waits, so those seconds overlap rather than add. There is
 *   deliberately no `combinedActiveSecs` in `derived.ts`.
 */

import {
	arr,
	arrOr,
	bool,
	boolOr,
	ContractError,
	counts,
	num,
	numOr,
	object,
	optStr,
	str,
	strItem,
	type Counts
} from './decode.js';

export interface Tokens {
	input: number;
	output: number;
	thinking: number;
	cache_read: number;
	cache_write: number;
}

/** Per-tool file-change audit. See `changes.by_tool` in the contract. */
export interface ToolChanges {
	calls: number;
	files_touched: number;
	lines_added: number;
	lines_deleted: number;
	/**
	 * How many of `calls` gave no readable **line counts** — not "unread".
	 *
	 * `calls` is the total and `opaque` is a subset of it, so the difference
	 * is meaningful and a renderer must not treat `opaque === calls` as
	 * "recovered nothing". Since sprint 013 a shell call whose command
	 * provably wrote nothing is neither readable nor opaque.
	 */
	opaque: number;
}

export interface Changes {
	files_touched: number;
	lines_added: number;
	lines_deleted: number;
	/**
	 * Calls that could have changed files and left no recoverable diff.
	 *
	 * Not every shell call: since sprint 013 the command string is read, and
	 * one that provably wrote nothing is a known zero rather than an unknown.
	 */
	opaque_edits: number;
	by_tool: Record<string, ToolChanges>;
}

export interface Bucket {
	records: number;
	tool_calls: number;
	tool_failures: number;
	user_turns: number;
	output_tokens: number;
}

export interface ActivitySpan {
	started: string;
	ended: string;
	secs: number;
	/** The idle gap before this span; `0` for the first. Idle has no buckets. */
	idle_before_secs: number;
	buckets: Bucket[];
}

export interface Activity {
	/** A property of the session, not the renderer: two renderings agree. */
	bucket_secs: number;
	spans: ActivitySpan[];
}

/** The seven classes `mix` counts and an event's `class` names. */
export type ToolClass = 'read' | 'edit' | 'run' | 'org' | 'ask' | 'delegate' | 'other';

export type ToolMix = Record<ToolClass, number>;

export const TOOL_CLASSES: readonly ToolClass[] = [
	'read',
	'edit',
	'run',
	'org',
	'ask',
	'delegate',
	'other'
];

/** A tool mix, not an intent — see the contract's `kind` and `mix`. */
export type PhaseKind =
	'exploring' | 'implementing' | 'running' | 'filing' | 'delegating' | 'discussing' | 'mixed';

const PHASE_KINDS: readonly string[] = [
	'exploring',
	'implementing',
	'running',
	'filing',
	'delegating',
	'discussing',
	'mixed'
];

export interface Phase {
	started: string;
	ended: string;
	secs: number;
	/** Index into `activity.spans` — a phase never spans an idle break. */
	span: number;
	kind: PhaseKind;
	records: number;
	tool_calls: number;
	tool_failures: number;
	output_tokens: number;
	mix: ToolMix;
	/** Absent when the phase opens a resumed span: work picked up with nothing said. */
	opened_by?: string;
}

export interface Prompt {
	kind: 'prompt';
	at?: string;
	preview: string;
	truncated: boolean;
	attachments: number;
}

export interface Question {
	kind: 'question';
	at?: string;
	header?: string;
	question: string;
	options: string[];
	/** Absent when the transcript holds no answer — interrupted, not defaulted. */
	chosen?: string;
}

export type Involvement = Prompt | Question;

export interface Spawn {
	agent_id?: string;
	/** Absent when the spawn could not be joined to its `Agent` call. */
	subagent_type?: string;
	description?: string;
	model?: string;
	/** `true` when the numbers came from a `subagents/agent-*.jsonl` sidecar. */
	sidecar: boolean;
	started?: string;
	ended?: string;
	active_secs: number;
	records: number;
	skipped_lines: number;
	assistant_turns: number;
	tool_calls: Counts;
	tool_failures: Counts;
	tokens: Tokens;
	changes: Changes;
}

export interface DelegatedTotals {
	records: number;
	assistant_turns: number;
	tool_calls: Counts;
	tool_failures: Counts;
	tokens: Tokens;
	changes: Changes;
}

export interface Delegation {
	spawns: Spawn[];
	/** `Agent` calls with no transcript to read — an unknown, not a zero. */
	unjoined_spawns: number;
	inline_records: number;
	totals: DelegatedTotals;
}

export interface PhaseLabel {
	phase: number;
	label: string;
}

/**
 * The one model-written object. A consumer that ignores this key entirely gets
 * the document kagviz emitted before sprint 004 — which is what "additive"
 * means here, and why the phase labels are a parallel array keyed by index
 * rather than a field on `Phase`.
 */
export interface Labels {
	headline: string;
	phases: PhaseLabel[];
	model: string;
	prompt_version: string;
	/** sha256 over the facts with `labels` removed. */
	facts_digest: string;
	generated: string;
}

export interface Facts {
	session_id?: string;
	project?: string;
	cwd?: string;
	git_branch?: string;
	cli_versions: string[];
	models: Counts;
	started?: string;
	ended?: string;
	wall_secs: number;
	active_secs: number;
	idle_secs: number;
	records: number;
	/** Non-zero means every number here is partial, and a page must say so. */
	skipped_lines: number;
	assistant_turns: number;
	user_prompts: number;
	pasted_attachments: number;
	ask_user_questions: number;
	skills: string[];
	subagents: string[];
	subagent_transcripts: number;
	tool_calls: Counts;
	tool_failures: Counts;
	tokens: Tokens;
	changes: Changes;
	activity: Activity;
	phases: Phase[];
	user_involvement: Involvement[];
	delegation: Delegation;
	labels?: Labels;
}

export function decodeFacts(raw: unknown, path = 'facts'): Facts {
	const o = object(raw, path);
	return {
		session_id: optStr(o, 'session_id', path),
		project: optStr(o, 'project', path),
		cwd: optStr(o, 'cwd', path),
		git_branch: optStr(o, 'git_branch', path),
		cli_versions: arrOr(o, 'cli_versions', path, strItem),
		models: counts(o, 'models', path),
		started: optStr(o, 'started', path),
		ended: optStr(o, 'ended', path),
		wall_secs: num(o, 'wall_secs', path),
		active_secs: num(o, 'active_secs', path),
		idle_secs: num(o, 'idle_secs', path),
		records: num(o, 'records', path),
		skipped_lines: num(o, 'skipped_lines', path),
		assistant_turns: num(o, 'assistant_turns', path),
		user_prompts: num(o, 'user_prompts', path),
		// Added after the first contract was written; a facts file from an
		// older kagviz has neither, and absent is a zero for a plain count.
		pasted_attachments: numOr(o, 'pasted_attachments', path, 0),
		ask_user_questions: numOr(o, 'ask_user_questions', path, 0),
		skills: arrOr(o, 'skills', path, strItem),
		subagents: arrOr(o, 'subagents', path, strItem),
		subagent_transcripts: numOr(o, 'subagent_transcripts', path, 0),
		tool_calls: counts(o, 'tool_calls', path),
		tool_failures: counts(o, 'tool_failures', path),
		tokens: decodeTokens(o['tokens'], `${path}.tokens`),
		changes: decodeChanges(o['changes'], `${path}.changes`),
		activity: decodeActivity(o['activity'], `${path}.activity`),
		phases: arr(o, 'phases', path, decodePhase),
		user_involvement: arr(o, 'user_involvement', path, decodeInvolvement),
		delegation: decodeDelegation(o['delegation'], `${path}.delegation`),
		labels: o['labels'] == null ? undefined : decodeLabels(o['labels'], `${path}.labels`)
	};
}

export function decodeTokens(raw: unknown, path: string): Tokens {
	const o = object(raw ?? {}, path);
	return {
		input: numOr(o, 'input', path, 0),
		output: numOr(o, 'output', path, 0),
		thinking: numOr(o, 'thinking', path, 0),
		cache_read: numOr(o, 'cache_read', path, 0),
		cache_write: numOr(o, 'cache_write', path, 0)
	};
}

function decodeToolChanges(raw: unknown, path: string): ToolChanges {
	const o = object(raw, path);
	return {
		calls: numOr(o, 'calls', path, 0),
		files_touched: numOr(o, 'files_touched', path, 0),
		lines_added: numOr(o, 'lines_added', path, 0),
		lines_deleted: numOr(o, 'lines_deleted', path, 0),
		opaque: numOr(o, 'opaque', path, 0)
	};
}

export function decodeChanges(raw: unknown, path: string): Changes {
	const o = object(raw ?? {}, path);
	const by_tool: Record<string, ToolChanges> = {};
	for (const [tool, v] of Object.entries(object(o['by_tool'] ?? {}, `${path}.by_tool`))) {
		by_tool[tool] = decodeToolChanges(v, `${path}.by_tool.${tool}`);
	}
	return {
		files_touched: numOr(o, 'files_touched', path, 0),
		lines_added: numOr(o, 'lines_added', path, 0),
		lines_deleted: numOr(o, 'lines_deleted', path, 0),
		opaque_edits: numOr(o, 'opaque_edits', path, 0),
		by_tool
	};
}

function decodeBucket(raw: unknown, path: string): Bucket {
	const o = object(raw, path);
	return {
		records: numOr(o, 'records', path, 0),
		tool_calls: numOr(o, 'tool_calls', path, 0),
		tool_failures: numOr(o, 'tool_failures', path, 0),
		user_turns: numOr(o, 'user_turns', path, 0),
		output_tokens: numOr(o, 'output_tokens', path, 0)
	};
}

function decodeSpan(raw: unknown, path: string): ActivitySpan {
	const o = object(raw, path);
	return {
		started: str(o, 'started', path),
		ended: str(o, 'ended', path),
		secs: num(o, 'secs', path),
		idle_before_secs: numOr(o, 'idle_before_secs', path, 0),
		buckets: arrOr(o, 'buckets', path, decodeBucket)
	};
}

export function decodeActivity(raw: unknown, path: string): Activity {
	const o = object(raw ?? {}, path);
	return {
		bucket_secs: numOr(o, 'bucket_secs', path, 0),
		spans: arrOr(o, 'spans', path, decodeSpan)
	};
}

export function decodeMix(raw: unknown, path: string): ToolMix {
	const o = object(raw ?? {}, path);
	const mix = {} as ToolMix;
	for (const c of TOOL_CLASSES) mix[c] = numOr(o, c, path, 0);
	return mix;
}

function decodePhase(raw: unknown, path: string): Phase {
	const o = object(raw, path);
	const kind = str(o, 'kind', path);
	if (!PHASE_KINDS.includes(kind)) {
		// A kind this app has never heard of is a kagviz newer than the app.
		// Failing loudly beats drawing it as one of the seven it does know.
		throw new ContractError(`${path}.kind`, `unknown phase kind ${JSON.stringify(kind)}`);
	}
	return {
		started: str(o, 'started', path),
		ended: str(o, 'ended', path),
		secs: num(o, 'secs', path),
		span: num(o, 'span', path),
		kind: kind as PhaseKind,
		records: numOr(o, 'records', path, 0),
		tool_calls: numOr(o, 'tool_calls', path, 0),
		tool_failures: numOr(o, 'tool_failures', path, 0),
		output_tokens: numOr(o, 'output_tokens', path, 0),
		mix: decodeMix(o['mix'], `${path}.mix`),
		opened_by: optStr(o, 'opened_by', path)
	};
}

function decodeInvolvement(raw: unknown, path: string): Involvement {
	const o = object(raw, path);
	const kind = str(o, 'kind', path);
	if (kind === 'prompt') {
		return {
			kind: 'prompt',
			at: optStr(o, 'at', path),
			preview: str(o, 'preview', path),
			truncated: bool(o, 'truncated', path),
			attachments: numOr(o, 'attachments', path, 0)
		};
	}
	if (kind === 'question') {
		return {
			kind: 'question',
			at: optStr(o, 'at', path),
			header: optStr(o, 'header', path),
			question: str(o, 'question', path),
			options: arrOr(o, 'options', path, strItem),
			chosen: optStr(o, 'chosen', path)
		};
	}
	throw new ContractError(`${path}.kind`, `unknown involvement kind ${JSON.stringify(kind)}`);
}

function decodeSpawn(raw: unknown, path: string): Spawn {
	const o = object(raw, path);
	return {
		agent_id: optStr(o, 'agent_id', path),
		subagent_type: optStr(o, 'subagent_type', path),
		description: optStr(o, 'description', path),
		model: optStr(o, 'model', path),
		sidecar: boolOr(o, 'sidecar', path, false),
		started: optStr(o, 'started', path),
		ended: optStr(o, 'ended', path),
		active_secs: numOr(o, 'active_secs', path, 0),
		records: numOr(o, 'records', path, 0),
		skipped_lines: numOr(o, 'skipped_lines', path, 0),
		assistant_turns: numOr(o, 'assistant_turns', path, 0),
		tool_calls: counts(o, 'tool_calls', path),
		tool_failures: counts(o, 'tool_failures', path),
		tokens: decodeTokens(o['tokens'], `${path}.tokens`),
		changes: decodeChanges(o['changes'], `${path}.changes`)
	};
}

export function decodeDelegation(raw: unknown, path: string): Delegation {
	const o = object(raw ?? {}, path);
	const totalsPath = `${path}.totals`;
	const t = object(o['totals'] ?? {}, totalsPath);
	return {
		spawns: arrOr(o, 'spawns', path, decodeSpawn),
		unjoined_spawns: numOr(o, 'unjoined_spawns', path, 0),
		inline_records: numOr(o, 'inline_records', path, 0),
		totals: {
			records: numOr(t, 'records', totalsPath, 0),
			assistant_turns: numOr(t, 'assistant_turns', totalsPath, 0),
			tool_calls: counts(t, 'tool_calls', totalsPath),
			tool_failures: counts(t, 'tool_failures', totalsPath),
			tokens: decodeTokens(t['tokens'], `${totalsPath}.tokens`),
			changes: decodeChanges(t['changes'], `${totalsPath}.changes`)
		}
	};
}

function decodeLabels(raw: unknown, path: string): Labels {
	const o = object(raw, path);
	return {
		headline: str(o, 'headline', path),
		phases: arrOr(o, 'phases', path, (v, p) => {
			const e = object(v, p);
			return { phase: num(e, 'phase', p), label: str(e, 'label', p) };
		}),
		model: str(o, 'model', path),
		prompt_version: str(o, 'prompt_version', path),
		facts_digest: str(o, 'facts_digest', path),
		generated: str(o, 'generated', path)
	};
}
