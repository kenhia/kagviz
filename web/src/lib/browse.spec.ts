import { describe, expect, it } from 'vitest';
import type { SessionEntry } from './contract/sessions.js';
import {
	DEFAULT_SORT,
	filter,
	hosts,
	projects,
	sessionHref,
	sort,
	what,
	whatIsWritten
} from './browse.js';

function row(over: Partial<SessionEntry> = {}): SessionEntry {
	return {
		host: 'kai',
		session_id: 'a',
		project: '-home-ken-src-x',
		cwd: '/home/ken/src/x',
		git_branch: 'main',
		started: '2026-08-20T10:00:00Z',
		ended: '2026-08-20T11:00:00Z',
		wall_secs: 3600,
		active_secs: 600,
		user_prompts: 2,
		assistant_turns: 8,
		tool_calls: 10,
		tool_failures: 0,
		files_touched: 1,
		lines_added: 5,
		lines_deleted: 1,
		opaque_edits: 0,
		output_tokens: 900,
		phases: 3,
		delegated_spawns: 0,
		skipped_lines: 0,
		models: ['claude-opus-5'],
		cli_versions: ['2.1.240'],
		facts: 'facts/kai/a.json',
		report: 'reports/kai/a.html',
		events: 'events/kai/a.json',
		...over
	};
}

describe('sessionHref', () => {
	/**
	 * The one that cost a deploy. `resolve()` from `$app/paths` returns
	 * `base + '#' + path`, and `base` is the *directory* the shell sits in — so
	 * the href came out `/kagviz/app#/s/…`, copyparty served that directory as
	 * a file listing, and following a row left the app. A bare fragment
	 * resolves against `document.baseURI`, which under hash routing is always
	 * the shell.
	 */
	it('is relative to the document, never rooted at the deployment directory', () => {
		const href = sessionHref(row({ host: 'kubs0', session_id: 'b7' }));
		expect(href).toBe('#/s/kubs0/b7');
		expect(href.startsWith('#')).toBe(true);
		expect(href).not.toMatch(/^\//);
	});

	it('escapes a host or id that would otherwise change the route', () => {
		expect(sessionHref(row({ host: 'a/b', session_id: 'c d' }))).toBe('#/s/a%2Fb/c%20d');
	});
});

describe('filtering', () => {
	const rows = [
		row({ session_id: 'a', host: 'kai', project: '-home-ken-src-x' }),
		row({ session_id: 'b', host: 'cleo', project: '-c-users-ken-y', cwd: 'C:\\Users\\ken\\y' }),
		row({
			session_id: 'c',
			host: 'kai',
			project: '-home-ken-src-x',
			opened_by: 'fix the flaky test'
		})
	];

	it('lists the hosts and projects actually present, sorted', () => {
		expect(hosts(rows)).toEqual(['cleo', 'kai']);
		expect(projects(rows)).toEqual(['-c-users-ken-y', '-home-ken-src-x']);
	});

	it('narrows by host and project independently', () => {
		expect(filter(rows, { host: 'kai', project: '', text: '' }).map((r) => r.session_id)).toEqual([
			'a',
			'c'
		]);
		expect(
			filter(rows, { host: '', project: '-c-users-ken-y', text: '' }).map((r) => r.session_id)
		).toEqual(['b']);
	});

	it('searches the id, cwd, branch and what the session was about', () => {
		expect(filter(rows, { host: '', project: '', text: 'flaky' }).map((r) => r.session_id)).toEqual(
			['c']
		);
		expect(
			filter(rows, { host: '', project: '', text: 'C:\\Users' }).map((r) => r.session_id)
		).toEqual(['b']);
		expect(filter(rows, { host: '', project: '', text: '   ' })).toHaveLength(3);
	});
});

describe('sorting', () => {
	it('defaults to newest first, as sessions.json itself is ordered', () => {
		const rows = [
			row({ session_id: 'old', started: '2026-08-01T00:00:00Z' }),
			row({ session_id: 'new', started: '2026-08-20T00:00:00Z' })
		];
		expect(sort(rows, DEFAULT_SORT).map((r) => r.session_id)).toEqual(['new', 'old']);
	});

	it('is a total order, so the table never reshuffles between renders', () => {
		const rows = [
			row({ session_id: 'b', active_secs: 60 }),
			row({ session_id: 'a', active_secs: 60 }),
			row({ session_id: 'c', active_secs: 60 })
		];
		const once = sort(rows, { key: 'active', desc: true }).map((r) => r.session_id);
		const again = sort([...rows].reverse(), { key: 'active', desc: true }).map((r) => r.session_id);
		expect(once).toEqual(again);
		expect(once).toEqual(['a', 'b', 'c']);
	});

	it('sorts by every column it offers a header for', () => {
		const rows = [row({ session_id: 'a', tool_calls: 1 }), row({ session_id: 'b', tool_calls: 9 })];
		expect(sort(rows, { key: 'tools', desc: true })[0].session_id).toBe('b');
		expect(sort(rows, { key: 'tools', desc: false })[0].session_id).toBe('a');
	});
});

describe('what a session was about', () => {
	it('prefers the written headline and marks it as written', () => {
		const r = row({ headline: 'Closed the undercount.', opened_by: 'fix the counts' });
		expect(what(r)).toBe('Closed the undercount.');
		expect(whatIsWritten(r)).toBe(true);
	});

	it('falls back to what the user opened with, which was not written', () => {
		const r = row({ opened_by: 'fix the counts' });
		expect(what(r)).toBe('fix the counts');
		expect(whatIsWritten(r)).toBe(false);
	});

	it('says nothing rather than inventing a subject', () => {
		expect(what(row())).toBe('');
		expect(whatIsWritten(row())).toBe(false);
	});
});
