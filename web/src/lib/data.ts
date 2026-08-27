/**
 * Where the app reads its four documents from, and how.
 *
 * The bundle is deployed to `derived/app/` and served by copyparty at
 * `/kagviz/app/index.html`; the data sits one level up, at `/kagviz/`. So the
 * derived root is `../` **relative to the document**, not to the route — which
 * is the second reason the router is hash-based: the document URL never
 * changes as you navigate, so a relative fetch resolves the same way on the
 * browser page and on a session page. A path-routed SPA at
 * `/kagviz/app/s/kai/<id>` would have to know how deep it was.
 *
 * `KAGVIZ_DERIVED` overrides it for `vite dev` (there is no derived tree beside
 * the dev server) — see `web/README.md`.
 */

import { decodeSessions, type SessionsIndex } from './contract/sessions.js';
import { decodeSyncStatus, type SyncStatus } from './contract/sessions.js';
import { decodeFacts, type Facts } from './contract/facts.js';
import { decodeEvents, type EventsDocument } from './contract/events.js';
import { decodeCalls, type CallsDocument } from './contract/calls.js';
import { ContractError, parse } from './contract/decode.js';

/** Trailing slash always, so `new URL(path, root)` behaves. */
export const DERIVED_ROOT: string = import.meta.env.VITE_KAGVIZ_DERIVED || '../';

function url(path: string): string {
	return new URL(path, new URL(DERIVED_ROOT, document.baseURI)).toString();
}

/**
 * Fetch and decode, or throw something a person can act on.
 *
 * A 404 and a document this app cannot read are different failures and say so
 * differently: one means the derive has not reached this session, the other
 * means kagviz and the app have drifted. Neither renders as an empty panel.
 */
async function load<T>(path: string, decode: (raw: unknown, path: string) => T): Promise<T> {
	const target = url(path);
	let res: Response;
	try {
		res = await fetch(target);
	} catch (e) {
		throw new Error(`could not reach ${path}: ${e instanceof Error ? e.message : String(e)}`, {
			cause: e
		});
	}
	if (!res.ok) {
		throw new Error(`${path} — ${res.status} ${res.statusText}`);
	}
	const text = await res.text();
	try {
		return decode(parse(text, path), path);
	} catch (e) {
		if (e instanceof ContractError) {
			throw new Error(
				`${path} is not a document this app understands (${e.message}). ` +
					`The app and the kagviz that derived this tree have drifted — see docs/facts-contract.md.`,
				{ cause: e }
			);
		}
		throw e;
	}
}

export function loadSessions(): Promise<SessionsIndex> {
	return load('sessions.json', decodeSessions);
}

export function loadFacts(host: string, id: string): Promise<Facts> {
	return load(`facts/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`, decodeFacts);
}

export function loadEvents(host: string, id: string): Promise<EventsDocument> {
	return load(`events/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`, decodeEvents);
}

/**
 * The calls document — the payload tier, fetched **lazily and only on
 * demand**.
 *
 * Never alongside the events, and never speculatively. It is ~4.5× the events
 * at the median (190 KB against 42 KB), a consumer that only wants the
 * timeline must not pay for it, and it is the one document a tree may
 * legitimately not have: `sessions.json` links `calls` only where `derive`
 * was asked to write it. Call this when a reader opens a call, not before —
 * and only when the index gave you a path.
 */
export function loadCalls(host: string, id: string): Promise<CallsDocument> {
	return load(`calls/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`, decodeCalls);
}

export interface Progress {
	/** Bytes read so far. */
	read: number;
	/** `Content-Length`, when the server sent one — absent, never guessed. */
	total?: number;
}

/**
 * The events document, read with its size visible.
 *
 * The facts are ~100 KB; a twelve-hour session's events run to megabytes (2.6
 * MB on the corpus's worst). A panel that sits blank for that long reads as
 * broken, so `#1639` asks for the size and the progress rather than a
 * spinner — and a server that sends no `Content-Length` leaves `total`
 * absent instead of being given an invented one to divide by.
 */
export async function loadEventsProgressively(
	host: string,
	id: string,
	onProgress: (p: Progress) => void
): Promise<EventsDocument> {
	const path = `events/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`;
	const target = url(path);
	let res: Response;
	try {
		res = await fetch(target);
	} catch (e) {
		throw new Error(`could not reach ${path}: ${e instanceof Error ? e.message : String(e)}`, {
			cause: e
		});
	}
	if (!res.ok) throw new Error(`${path} — ${res.status} ${res.statusText}`);

	const len = Number(res.headers.get('content-length'));
	const total = Number.isFinite(len) && len > 0 ? len : undefined;
	let text: string;
	if (!res.body) {
		text = await res.text();
		onProgress({ read: text.length, total });
	} else {
		const reader = res.body.getReader();
		const decoder = new TextDecoder();
		const parts: string[] = [];
		let read = 0;
		for (;;) {
			const { done, value } = await reader.read();
			if (done) break;
			read += value.byteLength;
			parts.push(decoder.decode(value, { stream: true }));
			onProgress({ read, total });
		}
		parts.push(decoder.decode());
		text = parts.join('');
	}
	try {
		return decodeEvents(parse(text, path), path);
	} catch (e) {
		if (e instanceof ContractError) {
			throw new Error(
				`${path} is not a document this app understands (${e.message}). ` +
					`The app and the kagviz that derived this tree have drifted — see docs/facts-contract.md.`,
				{ cause: e }
			);
		}
		throw e;
	}
}

/**
 * The sync status, or `undefined` when the tree carries none.
 *
 * Absent is not "everything was reached": the page says "no sync status" in
 * the place the host list would be, for the same reason the collector writes
 * the file at all.
 */
export async function loadSyncStatus(): Promise<SyncStatus | undefined> {
	try {
		return await load('sync-status.json', decodeSyncStatus);
	} catch {
		return undefined;
	}
}

/** The static report for a row, on the same served tree. */
export function reportUrl(path: string): string {
	return url(path);
}
