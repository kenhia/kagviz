/**
 * Sorting and filtering for the session browser — pure functions over
 * `sessions.json` rows, so they can be tested without a DOM.
 *
 * Everything here is presentation. No row's numbers are recomputed: the index
 * already carries them, copied from the facts, and this only decides which
 * rows are shown and in what order.
 */

import type { SessionEntry } from './contract/sessions.js';

export type SortKey =
	'started' | 'host' | 'project' | 'active' | 'prompts' | 'tools' | 'files' | 'what';

export interface Sort {
	key: SortKey;
	/** `true` for largest/newest first, which is the default for every column. */
	desc: boolean;
}

export interface Filter {
	host: string;
	project: string;
	text: string;
}

export const NO_FILTER: Filter = { host: '', project: '', text: '' };

/** `sessions.json` orders by `started` newest first; so does the table. */
export const DEFAULT_SORT: Sort = { key: 'started', desc: true };

/** What a row is "about", for the What column and the text filter. */
export function what(row: SessionEntry): string {
	return row.headline ?? row.opened_by ?? '';
}

/** True when the What column is showing model-written text, which must be marked. */
export function whatIsWritten(row: SessionEntry): boolean {
	return row.headline !== undefined;
}

export function hosts(rows: SessionEntry[]): string[] {
	return [...new Set(rows.map((r) => r.host))].sort();
}

export function projects(rows: SessionEntry[]): string[] {
	return [...new Set(rows.map((r) => r.project).filter((p): p is string => !!p))].sort();
}

export function filter(rows: SessionEntry[], f: Filter): SessionEntry[] {
	const needle = f.text.trim().toLowerCase();
	return rows.filter((r) => {
		if (f.host && r.host !== f.host) return false;
		if (f.project && r.project !== f.project) return false;
		if (!needle) return true;
		return haystack(r).includes(needle);
	});
}

function haystack(r: SessionEntry): string {
	return [r.session_id, r.host, r.project ?? '', r.cwd ?? '', r.git_branch ?? '', what(r)]
		.join(' ')
		.toLowerCase();
}

/**
 * A total order, always. Ties break on `started` then `session_id` so the
 * table never reshuffles between renders of the same data — the same argument
 * `sessions.json`'s own ordering makes one level down.
 */
export function sort(rows: SessionEntry[], s: Sort): SessionEntry[] {
	const dir = s.desc ? -1 : 1;
	return [...rows].sort((a, b) => cmp(a, b, s.key) * dir || tiebreak(a, b));
}

function tiebreak(a: SessionEntry, b: SessionEntry): number {
	return (
		(b.started ?? '').localeCompare(a.started ?? '') || a.session_id.localeCompare(b.session_id)
	);
}

function cmp(a: SessionEntry, b: SessionEntry, key: SortKey): number {
	switch (key) {
		case 'started':
			// An undated session sorts last under either direction rather than
			// leading the table: it is the one row whose place is unknown.
			return (a.started ?? '').localeCompare(b.started ?? '');
		case 'host':
			return a.host.localeCompare(b.host);
		case 'project':
			return (a.project ?? a.cwd ?? '').localeCompare(b.project ?? b.cwd ?? '');
		case 'active':
			return a.active_secs - b.active_secs;
		case 'prompts':
			return a.user_prompts - b.user_prompts;
		case 'tools':
			return a.tool_calls - b.tool_calls;
		case 'files':
			return a.files_touched - b.files_touched;
		case 'what':
			return what(a).localeCompare(what(b));
	}
}

/**
 * The route a row links to — a bare fragment, relative to the document.
 *
 * **Not** `resolve()` from `$app/paths`, which is the obvious-looking choice
 * and is wrong here. It returns `base + '#' + path`, where `base` is the
 * runtime-computed directory (`/kagviz/app`), so the href becomes an absolute
 * `/kagviz/app#/s/…` — a link to the *directory*, not to `index.html` in it.
 * copyparty serves a folder's listing rather than its `index.html` unless told
 * otherwise (`docs/collection.md` records the same trap for `/kagviz/`), so
 * following one of those links leaves the app for a file listing.
 *
 * A bare `#/…` resolves against `document.baseURI`, which is the shell itself
 * and never changes under hash routing. That is the whole reason the router is
 * hash-based, and it is the reason this is a string and not a call.
 */
export function sessionHref(row: SessionEntry): string {
	return `#/s/${encodeURIComponent(row.host)}/${encodeURIComponent(row.session_id)}`;
}
