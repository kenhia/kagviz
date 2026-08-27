/**
 * `sessions.json` — the cross-host index, and the file this app reads *first*
 * to choose a session before fetching its facts.
 *
 * A second contract, not part of the facts document: written by `kagviz
 * derive` into `derived/`, one row per session across every mirrored host.
 * Every figure is copied or summed from that session's facts; nothing here is
 * computed from a transcript directly and nothing is inferred.
 *
 * `tool_calls` and `tool_failures` are the session's **own** tier, summed —
 * delegated work is not folded in, and `delegated_spawns` says how many agents
 * there were.
 */

import { arrOr, num, numOr, object, optStr, str, strItem, type Counts } from './decode.js';

export interface SessionEntry {
	/** The mirror the session came from — not in the facts, which are host-agnostic. */
	host: string;
	session_id: string;
	project?: string;
	cwd?: string;
	git_branch?: string;
	started?: string;
	ended?: string;
	wall_secs: number;
	active_secs: number;
	user_prompts: number;
	assistant_turns: number;
	tool_calls: number;
	tool_failures: number;
	files_touched: number;
	lines_added: number;
	lines_deleted: number;
	/** Non-zero: the line deltas above are a floor, not a total. */
	opaque_edits: number;
	output_tokens: number;
	phases: number;
	delegated_spawns: number;
	skipped_lines: number;
	models: string[];
	cli_versions: string[];
	/** The first non-empty prompt preview. **Absent** when there is none. */
	opened_by?: string;
	/** `labels.headline` — written by a model, not counted. **Absent** otherwise. */
	headline?: string;
	/** Paths relative to the derived root, which is the served root. */
	facts: string;
	report: string;
	events: string;
	/**
	 * The calls document — the payload tier. **Absent when the tree carries
	 * none**, which is the default state and the whole point: `derive` writes
	 * call text only when asked, so this field being missing is the signal not
	 * to offer to open one. Added in 015.
	 */
	calls?: string;
	source_digest?: string;
	kagviz?: string;
}

/** An object holding the array, so a top-level field can be added later. */
export interface SessionsIndex {
	sessions: SessionEntry[];
}

export function decodeSessions(raw: unknown, path = 'sessions.json'): SessionsIndex {
	const o = object(raw, path);
	return { sessions: arrOr(o, 'sessions', path, decodeSessionEntry) };
}

export function decodeSessionEntry(raw: unknown, path: string): SessionEntry {
	const o = object(raw, path);
	return {
		host: str(o, 'host', path),
		session_id: str(o, 'session_id', path),
		project: optStr(o, 'project', path),
		cwd: optStr(o, 'cwd', path),
		git_branch: optStr(o, 'git_branch', path),
		started: optStr(o, 'started', path),
		ended: optStr(o, 'ended', path),
		wall_secs: num(o, 'wall_secs', path),
		active_secs: num(o, 'active_secs', path),
		user_prompts: num(o, 'user_prompts', path),
		assistant_turns: num(o, 'assistant_turns', path),
		tool_calls: num(o, 'tool_calls', path),
		tool_failures: num(o, 'tool_failures', path),
		files_touched: num(o, 'files_touched', path),
		lines_added: num(o, 'lines_added', path),
		lines_deleted: num(o, 'lines_deleted', path),
		opaque_edits: num(o, 'opaque_edits', path),
		output_tokens: num(o, 'output_tokens', path),
		phases: num(o, 'phases', path),
		delegated_spawns: num(o, 'delegated_spawns', path),
		skipped_lines: num(o, 'skipped_lines', path),
		models: arrOr(o, 'models', path, strItem),
		cli_versions: arrOr(o, 'cli_versions', path, strItem),
		opened_by: optStr(o, 'opened_by', path),
		headline: optStr(o, 'headline', path),
		facts: str(o, 'facts', path),
		report: str(o, 'report', path),
		// Added in 009. A derived tree written before it has no `events`, and
		// an empty string is what `#[serde(default)]` gives on the Rust side.
		events: optStr(o, 'events', path) ?? '',
		// Absent means the tree has no call text — never a reason to guess a
		// path and fetch it. Left `undefined` rather than defaulted to ''.
		calls: optStr(o, 'calls', path),
		source_digest: optStr(o, 'source_digest', path),
		kagviz: optStr(o, 'kagviz', path)
	};
}

/**
 * `sync-status.json` — which hosts the last sync reached. Read tolerantly:
 * every field optional, so a script-shaped file can never take the page down.
 * A host recorded `unreachable` must stay **visible**: "not reached" is not
 * "nothing new", and that distinction is the whole reason the file exists.
 */
export interface SyncStatus {
	ran_at?: string;
	hosts: Record<string, HostSync>;
}

export interface HostSync {
	status?: string;
	transferred?: number;
	secs?: number;
	note?: string;
}

export function decodeSyncStatus(raw: unknown, path = 'sync-status.json'): SyncStatus {
	const o = object(raw, path);
	const hosts: Record<string, HostSync> = {};
	for (const [host, v] of Object.entries(object(o['hosts'] ?? {}, `${path}.hosts`))) {
		const h = object(v, `${path}.hosts.${host}`);
		hosts[host] = {
			status: optStr(h, 'status', `${path}.hosts.${host}`),
			transferred: numOr(h, 'transferred', `${path}.hosts.${host}`, 0),
			secs: numOr(h, 'secs', `${path}.hosts.${host}`, 0),
			note: optStr(h, 'note', `${path}.hosts.${host}`)
		};
	}
	return { ran_at: optStr(o, 'ran_at', path), hosts };
}

export type { Counts };
